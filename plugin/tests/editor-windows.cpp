#include "Instrument.h"
#include "Editor.h"
#include <windows.h>
#include <filesystem>
#include <iostream>
int main(int argc,char** argv){
  if(argc!=2)return 1;_putenv_s("RUSTIAS_UI_CHECK","1");
  clap_host_t host={CLAP_VERSION,nullptr,"Silent editor check","Rustias","","0.1.0",[](const clap_host_t*,const char*)->const void*{return nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
  Instrument instrument(&host,argv[1]);if(!instrument.activate(48000)){std::cerr<<instrument.lastError;return 1;}
  auto window=CreateWindowExW(0,L"STATIC",L"Silent editor validation",WS_OVERLAPPEDWINDOW,0,0,1100,800,nullptr,nullptr,GetModuleHandleW(nullptr),nullptr);
  instrument.editor=std::make_unique<Editor>(instrument);if(!instrument.editor->parent(window))return 1;instrument.editor->show(true);
  const auto deadline=GetTickCount64()+30000;
  while(!instrument.editorReady&&instrument.lastError.empty()&&GetTickCount64()<deadline){MSG msg;while(PeekMessageW(&msg,nullptr,0,0,PM_REMOVE)){TranslateMessage(&msg);DispatchMessageW(&msg);}
    float l[256],r[256];float* channels[]={l,r};clap_audio_buffer_t out{};out.data32=channels;out.channel_count=2;clap_process_t p{};p.frames_count=256;p.audio_outputs=&out;p.audio_outputs_count=1;instrument.process(&p);Sleep(10);
  }
  if(!instrument.editorReady){std::cerr<<"Editor failed: "<<instrument.lastError;return 1;}
  instrument.editor->show(false);instrument.editor->parent(window);instrument.editor->show(true);instrument.editor.reset();DestroyWindow(window);
  std::cout<<"WebView2 editor, shared interface, WASM module, RDL/recording workers and IndexedDB initialized without audio hardware.\n";
}
