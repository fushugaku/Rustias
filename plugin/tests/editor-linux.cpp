// Real WebKitGTK initialization and XEmbed lifecycle; discarded audio buffers only.
#include "Instrument.h"
#include "Editor.h"
#include <gtk/gtk.h>
#include <gtk/gtkx.h>
#include <chrono>
#include <thread>
#include <iostream>
int main(int argc,char** argv){
  if(argc!=2)return 1;setenv("RUSTIAS_UI_CHECK","1",1);if(!gtk_init_check(nullptr,nullptr))return 1;
  clap_host_t host={CLAP_VERSION,nullptr,"Silent Linux editor check","Rustias","","0.2.1",[](const clap_host_t*,const char*)->const void*{return nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
  Instrument instrument(&host,argv[1]);if(!instrument.activate(48000)){std::cerr<<instrument.lastError;return 1;}
  auto window=gtk_window_new(GTK_WINDOW_TOPLEVEL),socket=gtk_socket_new();gtk_container_add(GTK_CONTAINER(window),socket);gtk_widget_show_all(window);
  const auto parent=reinterpret_cast<void*>(uintptr_t(gtk_socket_get_id(GTK_SOCKET(socket))));
  instrument.editor=std::make_unique<Editor>(instrument);if(!instrument.editor->parent(parent))return 1;instrument.editor->show(true);
  const auto deadline=std::chrono::steady_clock::now()+std::chrono::seconds(30);
  while(!instrument.editorReady&&instrument.lastError.empty()&&std::chrono::steady_clock::now()<deadline){
    float left[256],right[256];float* channels[]={left,right};clap_audio_buffer_t output{};output.data32=channels;output.channel_count=2;clap_process_t p{};p.frames_count=256;p.audio_outputs=&output;p.audio_outputs_count=1;instrument.process(&p);
    for(unsigned i=0;i<64&&g_main_context_pending(nullptr);i++)g_main_context_iteration(nullptr,FALSE);
    instrument.editor->poll();std::this_thread::sleep_for(std::chrono::milliseconds(10));
  }
  if(!instrument.editorReady){std::cerr<<"Editor failed: "<<instrument.lastError<<'\n';return 1;}
  for(int i=0;i<8;i++){instrument.editor->show(false);instrument.editor->size(900+i*10,650);instrument.editor->parent(parent);instrument.editor->show(true);instrument.editor->poll();}
  if(!instrument.lastError.empty()){std::cerr<<instrument.lastError;return 1;}
  instrument.editor.reset();gtk_widget_destroy(window);
  std::cout<<"WebKitGTK editor, XEmbed, workers, IndexedDB and hide/show initialized without audio hardware.\n";
}
