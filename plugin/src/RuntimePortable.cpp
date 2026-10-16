// Windows host: the unchanged worklet runs in QuickJS; its exact WASM runs in Wasmtime.
#include "Runtime.h"
#include <quickjs.h>
#include <wasmtime.h>
#include <array>
#include <fstream>
#include <stdexcept>
#include <cstring>
#include <algorithm>
#include <filesystem>
static std::string readFile(const std::string& path){std::ifstream f(std::filesystem::u8path(path),std::ios::binary);if(!f)throw std::runtime_error("Missing plugin resource: "+path);return {std::istreambuf_iterator<char>(f),{}};}
struct Runtime::Portable {
  Runtime& owner;
  JSRuntime* jsRuntime=nullptr;JSContext* ctx=nullptr;
  wasm_engine_t* engine=nullptr;wasmtime_store_t* store=nullptr;wasmtime_module_t* module=nullptr;
  wasmtime_context_t* wasmContext=nullptr;wasmtime_instance_t instance{};wasmtime_memory_t memory{};
  struct Function {wasmtime_func_t func;wasm_functype_t* type;};std::vector<Function> functions;
  JSValue buffer=JS_UNDEFINED;uint8_t* memoryBase=nullptr;size_t memorySize=0;
  std::vector<std::string> outgoing;
  Portable(Runtime& r):owner(r){}
  ~Portable(){if(ctx){JS_FreeValue(ctx,buffer);JS_FreeContext(ctx);}if(jsRuntime)JS_FreeRuntime(jsRuntime);for(auto& f:functions)wasm_functype_delete(f.type);if(store)wasmtime_store_delete(store);if(module)wasmtime_module_delete(module);if(engine)wasm_engine_delete(engine);}
  static Portable& get(JSContext* c){return *static_cast<Portable*>(JS_GetContextOpaque(c));}
  static std::string string(JSContext* c,JSValueConst v){size_t size=0;auto p=JS_ToCStringLen(c,&size,v);if(!p)return {};std::string s(p,size);JS_FreeCString(c,p);return s;}
  [[noreturn]] void fail(const std::string& message){owner.failure=true;owner.errorText=message;throw std::runtime_error(message);}
  void wasmError(wasmtime_error_t* error,wasm_trap_t* trap){
    if(!error&&!trap)return;wasm_byte_vec_t text;if(error){wasmtime_error_message(error,&text);wasmtime_error_delete(error);}else{wasm_trap_message(trap,&text);wasm_trap_delete(trap);}std::string message(text.data,text.size);wasm_byte_vec_delete(&text);fail(message);
  }
  JSValue checked(JSValue v){if(JS_IsException(v)){auto e=JS_GetException(ctx);auto message=string(ctx,e);JS_FreeValue(ctx,e);fail(message);}return v;}
  void evaluate(const std::string& code){auto v=checked(JS_Eval(ctx,code.data(),code.size(),"rustias-host.js",JS_EVAL_TYPE_GLOBAL));JS_FreeValue(ctx,v);}
  JSValue call(const char* name,int count,JSValueConst* args){auto global=JS_GetGlobalObject(ctx);auto fn=JS_GetPropertyStr(ctx,global,name);auto v=JS_Call(ctx,fn,global,count,args);JS_FreeValue(ctx,fn);JS_FreeValue(ctx,global);return checked(v);}
  static JSValue post(JSContext* c,JSValueConst,int n,JSValueConst* args){if(n)get(c).outgoing.push_back(string(c,args[0]));return JS_UNDEFINED;}
  static JSValue memoryBuffer(JSContext* c,JSValueConst,int,JSValueConst*){
    auto& s=get(c);auto base=wasmtime_memory_data(s.wasmContext,&s.memory);auto size=wasmtime_memory_data_size(s.wasmContext,&s.memory);
    if(s.memoryBase&&base!=s.memoryBase)return JS_ThrowInternalError(c,"The WASM memory reservation moved");
    if(size!=s.memorySize){JS_FreeValue(c,s.buffer);s.buffer=JS_NewArrayBuffer(c,base,size,0,nullptr,nullptr,false);s.memoryBase=base;s.memorySize=size;}
    return JS_DupValue(c,s.buffer);
  }
  static JSValue invoke(JSContext* c,JSValueConst,int argc,JSValueConst* argv,int magic){
    auto& s=get(c);auto& f=s.functions.at(magic);auto params=wasm_functype_params(f.type),results=wasm_functype_results(f.type);
    if(params->size>16||results->size>1)return JS_ThrowInternalError(c,"Unsupported WASM signature");
    std::array<wasmtime_val_t,16> args{};wasmtime_val_t result{};
    for(size_t i=0;i<params->size;i++){
      auto v=i<size_t(argc)?argv[i]:JS_UNDEFINED;
      switch(wasm_valtype_kind(params->data[i])){
        case WASM_I32:args[i].kind=WASMTIME_I32;JS_ToInt32(c,&args[i].of.i32,v);break;
        case WASM_I64:args[i].kind=WASMTIME_I64;JS_ToInt64(c,&args[i].of.i64,v);break;
        case WASM_F32:{double d=0;JS_ToFloat64(c,&d,v);args[i].kind=WASMTIME_F32;args[i].of.f32=float(d);break;}
        case WASM_F64:args[i].kind=WASMTIME_F64;JS_ToFloat64(c,&args[i].of.f64,v);break;
        default:return JS_ThrowInternalError(c,"Unsupported WASM argument");
      }
    }
    wasm_trap_t* trap=nullptr;auto error=wasmtime_func_call(s.wasmContext,&f.func,args.data(),params->size,&result,results->size,&trap);
    try{s.wasmError(error,trap);}catch(const std::exception& e){return JS_ThrowInternalError(c,"%s",e.what());}
    if(!results->size)return JS_UNDEFINED;
    switch(result.kind){case WASMTIME_I32:return JS_NewInt32(c,result.of.i32);case WASMTIME_I64:return JS_NewInt64(c,result.of.i64);case WASMTIME_F32:return JS_NewFloat64(c,result.of.f32);case WASMTIME_F64:return JS_NewFloat64(c,result.of.f64);default:return JS_ThrowInternalError(c,"Unsupported WASM result");}
  }
  void initialize(const std::string& resources,double rate){
    auto config=wasm_config_new();wasmtime_config_memory_reservation_set(config,uint64_t(1)<<32);wasmtime_config_memory_may_move_set(config,false);engine=wasm_engine_new_with_config(config);store=wasmtime_store_new(engine,nullptr,nullptr);wasmContext=wasmtime_store_context(store);
    auto bytes=readFile(resources+"/rustias.wasm");wasmError(wasmtime_module_new(engine,reinterpret_cast<const uint8_t*>(bytes.data()),bytes.size(),&module),nullptr);
    wasm_trap_t* trap=nullptr;auto error=wasmtime_instance_new(wasmContext,module,nullptr,0,&instance,&trap);wasmError(error,trap);
    jsRuntime=JS_NewRuntime();ctx=JS_NewContext(jsRuntime);JS_SetContextOpaque(ctx,this);
    auto global=JS_GetGlobalObject(ctx),exports=JS_NewObject(ctx);
    for(size_t i=0;;i++){
      char* name=nullptr;size_t length=0;wasmtime_extern_t item{};if(!wasmtime_instance_export_nth(wasmContext,&instance,i,&name,&length,&item))break;std::string key(name,length);
      if(item.kind==WASMTIME_EXTERN_FUNC){auto type=wasmtime_func_type(wasmContext,&item.of.func);int index=int(functions.size());functions.push_back({item.of.func,type});JS_SetPropertyStr(ctx,exports,key.c_str(),JS_NewCFunctionMagic(ctx,invoke,key.c_str(),int(wasm_functype_params(type)->size),JS_CFUNC_generic_magic,index));}
      else if(item.kind==WASMTIME_EXTERN_MEMORY)memory=item.of.memory;
      wasmtime_extern_delete(&item);
    }
    auto mem=JS_NewObject(ctx);auto bufferAtom=JS_NewAtom(ctx,"buffer");JS_DefinePropertyGetSet(ctx,mem,bufferAtom,JS_NewCFunction(ctx,memoryBuffer,"get buffer",0),JS_UNDEFINED,JS_PROP_CONFIGURABLE);JS_FreeAtom(ctx,bufferAtom);JS_SetPropertyStr(ctx,exports,"memory",mem);JS_SetPropertyStr(ctx,global,"__wasmExports",exports);
    JS_SetPropertyStr(ctx,global,"__nativePost",JS_NewCFunction(ctx,post,"__nativePost",1));JS_FreeValue(ctx,global);
    evaluate("globalThis.WebAssembly={Module:class{},Instance:class{constructor(){this.exports=globalThis.__wasmExports}}};globalThis.AudioWorkletProcessor=class{constructor(){this.port={postMessage:data=>__nativePost(JSON.stringify(data,(_k,v)=>ArrayBuffer.isView(v)?Array.from(v):v))}}};globalThis.registerProcessor=(_name,fn)=>globalThis.PluginProcessor=fn;");
    evaluate(readFile(resources+"/runtime.js"));JSValue args[]={JS_UNDEFINED,JS_NewFloat64(ctx,rate)};auto result=call("pluginBoot",2,args);JS_FreeValue(ctx,result);
  }
};
Runtime::Runtime(const std::string& resources,double rate):portable(std::make_unique<Portable>(*this)){portable->initialize(resources,rate);}
Runtime::~Runtime()=default;
void Runtime::message(const std::string& text){auto& s=*portable;auto value=s.checked(JS_ParseJSON(s.ctx,text.data(),text.size(),"host-message"));auto result=s.call("pluginMessage",1,&value);JS_FreeValue(s.ctx,value);JS_FreeValue(s.ctx,result);}
void Runtime::midi(unsigned char a,unsigned char b,unsigned char c){auto& s=*portable;JSValue args[]={JS_NewInt32(s.ctx,a),JS_NewInt32(s.ctx,b),JS_NewInt32(s.ctx,c)};auto r=s.call("pluginMidi",3,args);JS_FreeValue(s.ctx,r);}
void Runtime::parameter(unsigned index,double value){auto& s=*portable;JSValue args[]={JS_NewInt32(s.ctx,index),JS_NewFloat64(s.ctx,value)};auto r=s.call("pluginParameter",2,args);JS_FreeValue(s.ctx,r);}
void Runtime::render(float* left,float* right,unsigned frames){
  auto& s=*portable;JSValue count=JS_NewInt32(s.ctx,frames);auto result=s.call("pluginRender",1,&count);
  for(unsigned channel=0;channel<2;channel++){
    auto view=JS_GetPropertyUint32(s.ctx,result,channel);size_t offset=0,length=0,element=0,capacity=0;auto buffer=s.checked(JS_GetTypedArrayBuffer(s.ctx,view,&offset,&length,&element));auto ptr=JS_GetArrayBuffer(s.ctx,&capacity,buffer);
    if(!ptr||element!=sizeof(float)||length!=frames*sizeof(float)||offset+length>capacity)s.fail("Invalid host render buffer");std::memcpy(channel?right:left,ptr+offset,length);JS_FreeValue(s.ctx,buffer);JS_FreeValue(s.ctx,view);
  }JS_FreeValue(s.ctx,result);
}
std::vector<std::string> Runtime::takeMessages(){std::vector<std::string> result;result.swap(portable->outgoing);return result;}
std::string Runtime::stats(){auto& s=*portable;auto value=s.call("pluginStats",0,nullptr);auto text=Portable::string(s.ctx,value);JS_FreeValue(s.ctx,value);return text;}
