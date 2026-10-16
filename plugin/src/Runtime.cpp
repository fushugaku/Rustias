#include "Runtime.h"
#include <fstream>
#include <stdexcept>
#include <algorithm>
#include <cstring>

static std::string text(JSContextRef c,JSValueRef v){
  auto s=JSValueToStringCopy(c,v,nullptr);std::string r(JSStringGetMaximumUTF8CStringSize(s),'\0');
  auto n=JSStringGetUTF8CString(s,r.data(),r.size());r.resize(n?n-1:0);JSStringRelease(s);return r;
}
static std::string read(const std::string& p){std::ifstream f(p,std::ios::binary);if(!f)throw std::runtime_error("Missing plugin resource: "+p);return {std::istreambuf_iterator<char>(f),{}};}
void Runtime::check(JSValueRef e){if(e){failure=true;errorText=text(context,e);throw std::runtime_error(errorText);}}
void Runtime::evaluate(const std::string& source){auto s=JSStringCreateWithUTF8CString(source.c_str());JSValueRef e=nullptr;JSEvaluateScript(context,s,nullptr,nullptr,1,&e);JSStringRelease(s);check(e);}
JSObjectRef Runtime::function(const char* name){auto n=JSStringCreateWithUTF8CString(name);auto v=JSObjectGetProperty(context,JSContextGetGlobalObject(context),n,nullptr);JSStringRelease(n);return JSValueToObject(context,v,nullptr);}
JSValueRef Runtime::call(JSObjectRef fn,size_t n,const JSValueRef* a){JSValueRef e=nullptr;auto v=JSObjectCallAsFunction(context,fn,nullptr,n,a,&e);check(e);return v;}
JSValueRef Runtime::post(JSContextRef ctx,JSObjectRef,JSObjectRef,size_t count,const JSValueRef args[],JSValueRef*){
  auto self=static_cast<Runtime*>(JSObjectGetPrivate(JSContextGetGlobalObject(ctx)));
  if(self&&count)self->outgoing.push_back(text(ctx,args[0]));return JSValueMakeUndefined(ctx);
}
Runtime::Runtime(const std::string& resources,double rate){
  JSClassDefinition definition=kJSClassDefinitionEmpty;auto cls=JSClassCreate(&definition);context=JSGlobalContextCreate(cls);JSClassRelease(cls);JSObjectSetPrivate(JSContextGetGlobalObject(context),this);
  try{
    auto n=JSStringCreateWithUTF8CString("__nativePost");auto f=JSObjectMakeFunctionWithCallback(context,n,post);JSObjectSetProperty(context,JSContextGetGlobalObject(context),n,f,kJSPropertyAttributeReadOnly,nullptr);JSStringRelease(n);
    evaluate("globalThis.AudioWorkletProcessor=class{constructor(){this.port={postMessage:data=>__nativePost(JSON.stringify(data,(_k,v)=>ArrayBuffer.isView(v)?Array.from(v):v))}}};globalThis.registerProcessor=(_name,fn)=>globalThis.PluginProcessor=fn;");
    evaluate(read(resources+"/runtime.js"));auto data=read(resources+"/rustias.wasm");wasm.assign(data.begin(),data.end());
    JSValueRef e=nullptr;auto a=JSObjectMakeTypedArrayWithBytesNoCopy(context,kJSTypedArrayTypeUint8Array,wasm.data(),wasm.size(),[](void*,void*){},nullptr,&e);check(e);
    JSValueRef args[]={a,JSValueMakeNumber(context,rate)};call(function("pluginBoot"),2,args);
    receiver=function("pluginMessage");renderer=function("pluginRender");midiFn=function("pluginMidi");paramFn=function("pluginParameter");statsFn=function("pluginStats");
    for(auto f:{receiver,renderer,midiFn,paramFn,statsFn})JSValueProtect(context,f);
  }catch(...){JSGlobalContextRelease(context);context=nullptr;throw;}
}
Runtime::~Runtime(){if(context){for(auto f:{receiver,renderer,midiFn,paramFn,statsFn})JSValueUnprotect(context,f);JSGlobalContextRelease(context);}}
void Runtime::message(const std::string& json){auto s=JSStringCreateWithUTF8CString(json.c_str());auto v=JSValueMakeFromJSONString(context,s);JSStringRelease(s);if(!v)throw std::runtime_error("Invalid host message");call(receiver,1,&v);}
void Runtime::midi(unsigned char a,unsigned char b,unsigned char c){JSValueRef args[]={JSValueMakeNumber(context,a),JSValueMakeNumber(context,b),JSValueMakeNumber(context,c)};call(midiFn,3,args);}
void Runtime::parameter(unsigned i,double v){JSValueRef args[]={JSValueMakeNumber(context,i),JSValueMakeNumber(context,v)};call(paramFn,2,args);}
void Runtime::render(float* left,float* right,unsigned frames){
  JSValueRef n=JSValueMakeNumber(context,frames);auto result=JSValueToObject(context,call(renderer,1,&n),nullptr);JSValueProtect(context,result);
  for(unsigned channel=0;channel<2;channel++){
    auto array=JSValueToObject(context,JSObjectGetPropertyAtIndex(context,result,channel,nullptr),nullptr);
    if(JSObjectGetTypedArrayLength(context,array,nullptr)!=frames)throw std::runtime_error("Invalid render buffer");
    JSValueRef e=nullptr;auto ptr=JSObjectGetTypedArrayBytesPtr(context,array,&e);check(e);
    if(!ptr)throw std::runtime_error("Missing render buffer");
    std::memcpy(channel?right:left,ptr,frames*sizeof(float));
  }
  JSValueUnprotect(context,result);
}
std::vector<std::string> Runtime::takeMessages(){std::vector<std::string> r;r.swap(outgoing);return r;}
std::string Runtime::stats(){return text(context,call(statsFn,0,nullptr));}
