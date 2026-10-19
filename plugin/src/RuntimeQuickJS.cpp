// QuickJS hosts the shared sequencing/control layer. DSP is AOT native Rust.
#include "Runtime.h"
#include <quickjs.h>
#include <array>
#include <fstream>
#include <stdexcept>
#include <cstring>
#include <algorithm>
#include <filesystem>
#include <cmath>
static std::string readFile(const std::string& path){std::ifstream f(std::filesystem::u8path(path),std::ios::binary);if(!f)throw std::runtime_error("Missing plugin resource: "+path);return {std::istreambuf_iterator<char>(f),{}};}
struct Runtime::Portable {
  Runtime& owner;
  NativeEngine engine;
  JSRuntime* jsRuntime=nullptr;JSContext* ctx=nullptr;
  std::vector<std::string> outgoing;
  Portable(Runtime& r):owner(r){}
  ~Portable(){if(jsRuntime)JS_UpdateStackTop(jsRuntime);if(ctx)JS_FreeContext(ctx);if(jsRuntime)JS_FreeRuntime(jsRuntime);}
  static Portable& get(JSContext* c){return *static_cast<Portable*>(JS_GetContextOpaque(c));}
  static std::string string(JSContext* c,JSValueConst v){size_t size=0;auto p=JS_ToCStringLen(c,&size,v);if(!p)return {};std::string s(p,size);JS_FreeCString(c,p);return s;}
  [[noreturn]] void fail(const std::string& message){owner.failure=true;owner.errorText=message;throw std::runtime_error(message);}
  JSValue checked(JSValue v){if(JS_IsException(v)){auto e=JS_GetException(ctx);auto message=string(ctx,e);JS_FreeValue(ctx,e);fail(message);}return v;}
  void evaluate(const std::string& code){auto v=checked(JS_Eval(ctx,code.data(),code.size(),"rustias-host.js",JS_EVAL_TYPE_GLOBAL));JS_FreeValue(ctx,v);}
  JSValue call(const char* name,int count,JSValueConst* args){auto global=JS_GetGlobalObject(ctx);auto fn=JS_GetPropertyStr(ctx,global,name);auto v=JS_Call(ctx,fn,global,count,args);JS_FreeValue(ctx,fn);JS_FreeValue(ctx,global);return checked(v);}
  static JSValue post(JSContext* c,JSValueConst,int n,JSValueConst* args){if(n)get(c).outgoing.push_back(string(c,args[0]));return JS_UNDEFINED;}
  static JSValue invoke(JSContext* c,JSValueConst,int argc,JSValueConst* argv,int method){
    auto& s=get(c);std::array<double,16> args{};
    if(argc<0||size_t(argc)>args.size())return JS_ThrowInternalError(c,"Invalid native engine argument count");
    for(int i=0;i<argc;i++)if(JS_ToFloat64(c,&args[i],argv[i])<0)return JS_EXCEPTION;
    double result=0;if(!rustias_native_call(s.engine.handle,unsigned(method),args.data(),unsigned(argc),&result))return JS_ThrowInternalError(c,"Native engine rejected its API arguments");
    return JS_NewFloat64(c,result);
  }
  static JSValue readBuffer(JSContext* c,JSValueConst,int argc,JSValueConst* argv){
    if(argc!=3)return JS_ThrowInternalError(c,"Invalid native buffer request");
    uint32_t token;double length;if(JS_ToUint32(c,&token,argv[0])<0||JS_ToFloat64(c,&length,argv[1])<0)return JS_EXCEPTION;
    const bool floating=JS_ToBool(c,argv[2]);
    if(!std::isfinite(length)||length<0||length>32*1024*1024||std::floor(length)!=length)return JS_ThrowInternalError(c,"Invalid native buffer size");
    JSValue count=JS_NewInt32(c,int(length));auto view=JS_NewTypedArray(c,1,&count,floating?JS_TYPED_ARRAY_FLOAT32:JS_TYPED_ARRAY_UINT8);if(JS_IsException(view))return view;
    size_t offset=0,bytes=0,element=0,capacity=0;auto buffer=JS_GetTypedArrayBuffer(c,view,&offset,&bytes,&element);auto data=JS_GetArrayBuffer(c,&capacity,buffer);
    bool ok=data&&offset+bytes<=capacity&&rustias_native_read(get(c).engine.handle,token,data+offset,bytes);JS_FreeValue(c,buffer);
    if(!ok){JS_FreeValue(c,view);return JS_ThrowInternalError(c,"Expired or invalid native buffer");}return view;
  }
  static JSValue readBufferInto(JSContext* c,JSValueConst,int argc,JSValueConst* argv){
    if(argc!=2)return JS_ThrowInternalError(c,"Invalid native buffer read");
    uint32_t token;if(JS_ToUint32(c,&token,argv[0])<0)return JS_EXCEPTION;
    size_t offset=0,bytes=0,element=0,capacity=0;auto buffer=JS_GetTypedArrayBuffer(c,argv[1],&offset,&bytes,&element);if(JS_IsException(buffer))return buffer;auto data=JS_GetArrayBuffer(c,&capacity,buffer);
    bool ok=data&&offset<=capacity&&bytes<=capacity-offset&&rustias_native_read(get(c).engine.handle,token,data+offset,bytes);JS_FreeValue(c,buffer);
    if(!ok)return JS_ThrowInternalError(c,"Expired or invalid native buffer");return JS_UNDEFINED;
  }
  static JSValue writeBuffer(JSContext* c,JSValueConst,int argc,JSValueConst* argv){
    if(argc!=2)return JS_ThrowInternalError(c,"Invalid native buffer write");
    uint32_t token;if(JS_ToUint32(c,&token,argv[0])<0)return JS_EXCEPTION;
    size_t offset=0,bytes=0,element=0,capacity=0;auto buffer=JS_GetTypedArrayBuffer(c,argv[1],&offset,&bytes,&element);if(JS_IsException(buffer))return buffer;auto data=JS_GetArrayBuffer(c,&capacity,buffer);
    bool ok=data&&offset+bytes<=capacity&&rustias_native_write(get(c).engine.handle,token,data+offset,bytes);JS_FreeValue(c,buffer);
    if(!ok)return JS_ThrowInternalError(c,"Expired or invalid native buffer");return JS_UNDEFINED;
  }
  void initialize(const std::string& resources,double rate){
    jsRuntime=JS_NewRuntime();ctx=JS_NewContext(jsRuntime);JS_SetContextOpaque(ctx,this);
    auto global=JS_GetGlobalObject(ctx),api=JS_NewObject(ctx);
    for(uint32_t i=0;i<rustias_native_method_count();i++){
      auto name=rustias_native_method_name(i);JS_SetPropertyStr(ctx,api,name,JS_NewCFunctionMagic(ctx,invoke,name,int(rustias_native_method_arguments(i)),JS_CFUNC_generic_magic,int(i)));
    }
    JS_SetPropertyStr(ctx,api,"readBuffer",JS_NewCFunction(ctx,readBuffer,"readBuffer",3));
    JS_SetPropertyStr(ctx,api,"readBufferInto",JS_NewCFunction(ctx,readBufferInto,"readBufferInto",2));
    JS_SetPropertyStr(ctx,api,"writeBuffer",JS_NewCFunction(ctx,writeBuffer,"writeBuffer",2));
    JS_SetPropertyStr(ctx,global,"__nativePost",JS_NewCFunction(ctx,post,"__nativePost",1));JS_FreeValue(ctx,global);
    evaluate("globalThis.WebAssembly=undefined;globalThis.AudioWorkletProcessor=class{constructor(){this.port={postMessage:data=>__nativePost(JSON.stringify(data,(_k,v)=>ArrayBuffer.isView(v)?Array.from(v):v))}}};globalThis.registerProcessor=(_name,fn)=>globalThis.PluginProcessor=fn;");
    evaluate(readFile(resources+"/runtime.js"));JSValue args[]={api,JS_NewFloat64(ctx,rate)};auto result=call("pluginBoot",2,args);JS_FreeValue(ctx,result);JS_FreeValue(ctx,api);
  }
};
Runtime::Runtime(const std::string& resources,double rate):portable(std::make_unique<Portable>(*this)){portable->initialize(resources,rate);}
Runtime::~Runtime()=default;
void Runtime::message(const std::string& text){auto& s=*portable;JS_UpdateStackTop(s.jsRuntime);auto value=s.checked(JS_ParseJSON(s.ctx,text.data(),text.size(),"host-message"));auto result=s.call("pluginMessage",1,&value);JS_FreeValue(s.ctx,value);JS_FreeValue(s.ctx,result);}
void Runtime::midi(unsigned char a,unsigned char b,unsigned char c){auto& s=*portable;JS_UpdateStackTop(s.jsRuntime);JSValue args[]={JS_NewInt32(s.ctx,a),JS_NewInt32(s.ctx,b),JS_NewInt32(s.ctx,c)};auto r=s.call("pluginMidi",3,args);JS_FreeValue(s.ctx,r);}
void Runtime::parameter(unsigned index,double value){auto& s=*portable;JS_UpdateStackTop(s.jsRuntime);JSValue args[]={JS_NewInt32(s.ctx,index),JS_NewFloat64(s.ctx,value)};auto r=s.call("pluginParameter",2,args);JS_FreeValue(s.ctx,r);}
void Runtime::telemetry(bool enabled){if(telemetryEnabled==enabled)return;auto& s=*portable;JS_UpdateStackTop(s.jsRuntime);JSValue value=JS_NewBool(s.ctx,enabled);auto result=s.call("pluginTelemetry",1,&value);JS_FreeValue(s.ctx,result);telemetryEnabled=enabled;}
void Runtime::render(float* left,float* right,unsigned frames){
  auto& s=*portable;JS_UpdateStackTop(s.jsRuntime);JSValue count=JS_NewInt32(s.ctx,frames);auto result=s.call("pluginRender",1,&count);
  for(unsigned channel=0;channel<2;channel++){
    auto view=JS_GetPropertyUint32(s.ctx,result,channel);size_t offset=0,length=0,element=0,capacity=0;auto buffer=s.checked(JS_GetTypedArrayBuffer(s.ctx,view,&offset,&length,&element));auto ptr=JS_GetArrayBuffer(s.ctx,&capacity,buffer);
    if(!ptr||element!=sizeof(float)||length!=frames*sizeof(float)||offset+length>capacity)s.fail("Invalid host render buffer");std::memcpy(channel?right:left,ptr+offset,length);JS_FreeValue(s.ctx,buffer);JS_FreeValue(s.ctx,view);
  }JS_FreeValue(s.ctx,result);
}
std::vector<std::string> Runtime::takeMessages(){std::vector<std::string> result;result.swap(portable->outgoing);return result;}
std::vector<std::string> Runtime::takeRealtimeMessages(){return takeMessages();}
std::string Runtime::stats(){auto& s=*portable;JS_UpdateStackTop(s.jsRuntime);auto value=s.call("pluginStats",0,nullptr);auto text=Portable::string(s.ctx,value);JS_FreeValue(s.ctx,value);return text;}
