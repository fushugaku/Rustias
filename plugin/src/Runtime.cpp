#include "Runtime.h"
#include <fstream>
#include <stdexcept>
#include <algorithm>
#include <cstring>
#include <array>
#include <cmath>

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
static JSValueRef nativeError(JSContextRef ctx,JSValueRef* exception,const char* message){
  auto s=JSStringCreateWithUTF8CString(message);auto v=JSValueMakeString(ctx,s);JSStringRelease(s);*exception=JSObjectMakeError(ctx,1,&v,nullptr);return JSValueMakeUndefined(ctx);
}
JSValueRef Runtime::invoke(JSContextRef ctx,JSObjectRef fn,JSObjectRef,size_t count,const JSValueRef args[],JSValueRef* exception){
  auto b=static_cast<Binding*>(JSObjectGetPrivate(fn));std::array<double,16> values{};
  if(count>values.size())return nativeError(ctx,exception,"Invalid native engine argument count");
  for(size_t i=0;i<count;i++){values[i]=JSValueToNumber(ctx,args[i],exception);if(*exception)return JSValueMakeUndefined(ctx);}
  double result=0;if(!rustias_native_call(b->owner->engine.handle,b->method,values.data(),unsigned(count),&result))return nativeError(ctx,exception,"Native engine rejected its API arguments");
  return JSValueMakeNumber(ctx,result);
}
JSValueRef Runtime::readBuffer(JSContextRef ctx,JSObjectRef,JSObjectRef,size_t count,const JSValueRef args[],JSValueRef* exception){
  if(count!=3)return nativeError(ctx,exception,"Invalid native buffer request");
  auto self=static_cast<Runtime*>(JSObjectGetPrivate(JSContextGetGlobalObject(ctx)));
  double token=JSValueToNumber(ctx,args[0],exception),length=JSValueToNumber(ctx,args[1],exception);bool floating=JSValueToBoolean(ctx,args[2]);
  if(*exception)return JSValueMakeUndefined(ctx);
  if(!std::isfinite(length)||length<0||length>32*1024*1024||std::floor(length)!=length)return nativeError(ctx,exception,"Invalid native buffer size");
  auto view=JSObjectMakeTypedArray(ctx,floating?kJSTypedArrayTypeFloat32Array:kJSTypedArrayTypeUint8Array,size_t(length),exception);if(*exception)return JSValueMakeUndefined(ctx);
  auto bytes=static_cast<uint8_t*>(JSObjectGetTypedArrayBytesPtr(ctx,view,exception));if(*exception)return JSValueMakeUndefined(ctx);
  if(!rustias_native_read(self->engine.handle,uint32_t(token),bytes,size_t(length)*(floating?sizeof(float):1)))return nativeError(ctx,exception,"Expired or invalid native buffer");
  return view;
}
JSValueRef Runtime::readBufferInto(JSContextRef ctx,JSObjectRef,JSObjectRef,size_t count,const JSValueRef args[],JSValueRef* exception){
  if(count!=2)return nativeError(ctx,exception,"Invalid native buffer read");
  auto self=static_cast<Runtime*>(JSObjectGetPrivate(JSContextGetGlobalObject(ctx)));
  auto token=JSValueToNumber(ctx,args[0],exception);auto view=JSValueToObject(ctx,args[1],exception);if(*exception)return JSValueMakeUndefined(ctx);
  auto length=JSObjectGetTypedArrayByteLength(ctx,view,exception);if(*exception)return JSValueMakeUndefined(ctx);
  auto bytes=static_cast<uint8_t*>(JSObjectGetTypedArrayBytesPtr(ctx,view,exception));if(*exception)return JSValueMakeUndefined(ctx);
  if(!rustias_native_read(self->engine.handle,uint32_t(token),bytes,length))return nativeError(ctx,exception,"Expired or invalid native buffer");
  return JSValueMakeUndefined(ctx);
}
JSValueRef Runtime::writeBuffer(JSContextRef ctx,JSObjectRef,JSObjectRef,size_t count,const JSValueRef args[],JSValueRef* exception){
  if(count!=2)return nativeError(ctx,exception,"Invalid native buffer write");
  auto self=static_cast<Runtime*>(JSObjectGetPrivate(JSContextGetGlobalObject(ctx)));
  auto token=JSValueToNumber(ctx,args[0],exception);auto view=JSValueToObject(ctx,args[1],exception);if(*exception)return JSValueMakeUndefined(ctx);
  auto length=JSObjectGetTypedArrayByteLength(ctx,view,exception);auto bytes=static_cast<uint8_t*>(JSObjectGetTypedArrayBytesPtr(ctx,view,exception));if(*exception)return JSValueMakeUndefined(ctx);
  if(!rustias_native_write(self->engine.handle,uint32_t(token),bytes,length))return nativeError(ctx,exception,"Expired or invalid native buffer");
  return JSValueMakeUndefined(ctx);
}
Runtime::Runtime(const std::string& resources,double rate){
  JSClassDefinition definition=kJSClassDefinitionEmpty;auto cls=JSClassCreate(&definition);context=JSGlobalContextCreate(cls);JSClassRelease(cls);JSObjectSetPrivate(JSContextGetGlobalObject(context),this);
  try{
    auto n=JSStringCreateWithUTF8CString("__nativePost");auto f=JSObjectMakeFunctionWithCallback(context,n,post);JSObjectSetProperty(context,JSContextGetGlobalObject(context),n,f,kJSPropertyAttributeReadOnly,nullptr);JSStringRelease(n);
    evaluate("globalThis.WebAssembly=undefined;globalThis.AudioWorkletProcessor=class{constructor(){this.port={postMessage:data=>__nativePost(JSON.stringify(data,(_k,v)=>ArrayBuffer.isView(v)?Array.from(v):v))}}};globalThis.registerProcessor=(_name,fn)=>globalThis.PluginProcessor=fn;");
    evaluate(read(resources+"/runtime.js"));
    auto a=JSObjectMake(context,nullptr,nullptr);JSValueProtect(context,a);
    JSClassDefinition methodClass=kJSClassDefinitionEmpty;methodClass.className="NativeRustFunction";methodClass.callAsFunction=invoke;auto nativeClass=JSClassCreate(&methodClass);
    bindings.resize(rustias_native_method_count());
    for(unsigned i=0;i<bindings.size();i++){
      bindings[i]={this,i};auto name=JSStringCreateWithUTF8CString(rustias_native_method_name(i));auto method=JSObjectMake(context,nativeClass,&bindings[i]);
      JSObjectSetProperty(context,a,name,method,kJSPropertyAttributeReadOnly,nullptr);JSStringRelease(name);
    }JSClassRelease(nativeClass);
    for(auto pair:{std::pair{"readBuffer",readBuffer},std::pair{"readBufferInto",readBufferInto},std::pair{"writeBuffer",writeBuffer}}){auto name=JSStringCreateWithUTF8CString(pair.first);auto method=JSObjectMakeFunctionWithCallback(context,name,pair.second);JSObjectSetProperty(context,a,name,method,kJSPropertyAttributeReadOnly,nullptr);JSStringRelease(name);}
    JSValueRef args[]={a,JSValueMakeNumber(context,rate)};call(function("pluginBoot"),2,args);
    JSValueUnprotect(context,a);
    receiver=function("pluginMessage");renderer=function("pluginRender");midiFn=function("pluginMidi");paramFn=function("pluginParameter");statsFn=function("pluginStats");telemetryFn=function("pluginTelemetry");
    for(auto f:{receiver,renderer,midiFn,paramFn,statsFn,telemetryFn})JSValueProtect(context,f);
  }catch(...){JSGlobalContextRelease(context);context=nullptr;throw;}
}
Runtime::~Runtime(){if(context){for(auto f:{receiver,renderer,midiFn,paramFn,statsFn,telemetryFn})JSValueUnprotect(context,f);JSGlobalContextRelease(context);}}
void Runtime::message(const std::string& json){auto s=JSStringCreateWithUTF8CString(json.c_str());auto v=JSValueMakeFromJSONString(context,s);JSStringRelease(s);if(!v)throw std::runtime_error("Invalid host message");call(receiver,1,&v);}
void Runtime::midi(unsigned char a,unsigned char b,unsigned char c){JSValueRef args[]={JSValueMakeNumber(context,a),JSValueMakeNumber(context,b),JSValueMakeNumber(context,c)};call(midiFn,3,args);}
void Runtime::parameter(unsigned i,double v){JSValueRef args[]={JSValueMakeNumber(context,i),JSValueMakeNumber(context,v)};call(paramFn,2,args);}
void Runtime::telemetry(bool enabled){if(telemetryEnabled==enabled)return;JSValueRef value=JSValueMakeBoolean(context,enabled);call(telemetryFn,1,&value);telemetryEnabled=enabled;}
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
