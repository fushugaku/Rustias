#pragma once
#ifndef RUSTIAS_QUICKJS_RUNTIME
#include <JavaScriptCore/JavaScriptCore.h>
#endif
#include "NativeEngine.h"
#include <string>
#include <vector>
#include <memory>

class Runtime {
public:
  explicit Runtime(const std::string& resources,double rate);
  ~Runtime();
  Runtime(const Runtime&)=delete;
  Runtime& operator=(const Runtime&)=delete;
  void message(const std::string& json);
  void midi(unsigned char a,unsigned char b,unsigned char c);
  void parameter(unsigned index,double value);
  void render(float* left,float* right,unsigned frames);
  std::vector<std::string> takeMessages();
  std::string stats();
  bool failed()const{return failure;}
  const std::string& error()const{return errorText;}
private:
#ifdef RUSTIAS_QUICKJS_RUNTIME
  struct Portable;
  std::unique_ptr<Portable> portable;
#else
  JSGlobalContextRef context=nullptr;
  JSObjectRef receiver=nullptr,renderer=nullptr,midiFn=nullptr,paramFn=nullptr,statsFn=nullptr;
  std::vector<std::string> outgoing;
  NativeEngine engine;
  struct Binding { Runtime* owner; unsigned method; };
  std::vector<Binding> bindings;
  void check(JSValueRef exception);
  JSObjectRef function(const char* name);
  JSValueRef call(JSObjectRef fn,size_t count,const JSValueRef* values);
  void evaluate(const std::string& source);
  static JSValueRef post(JSContextRef,JSObjectRef,JSObjectRef,size_t,const JSValueRef[],JSValueRef*);
  static JSValueRef invoke(JSContextRef,JSObjectRef,JSObjectRef,size_t,const JSValueRef[],JSValueRef*);
  static JSValueRef readBuffer(JSContextRef,JSObjectRef,JSObjectRef,size_t,const JSValueRef[],JSValueRef*);
  static JSValueRef writeBuffer(JSContextRef,JSObjectRef,JSObjectRef,size_t,const JSValueRef[],JSValueRef*);
#endif
  std::string errorText;
  bool failure=false;
};
