#pragma once
#include <cstddef>
#include <cstdint>

// Rust static-library ABI. No WASM runtime or linear-memory emulation.
extern "C" {
void* rustias_native_create();
void rustias_native_destroy(void*);
uint32_t rustias_native_method_count();
const char* rustias_native_method_name(uint32_t);
uint32_t rustias_native_method_arguments(uint32_t);
bool rustias_native_call(void*,uint32_t,const double*,uint32_t,double*);
bool rustias_native_read(const void*,uint32_t,uint8_t*,size_t);
bool rustias_native_write(void*,uint32_t,const uint8_t*,size_t);
}

class NativeEngine {
public:
  NativeEngine():handle(rustias_native_create()){}
  ~NativeEngine(){rustias_native_destroy(handle);}
  NativeEngine(const NativeEngine&)=delete;
  NativeEngine& operator=(const NativeEngine&)=delete;
  void* handle;
};
