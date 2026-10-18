#include "LinuxEditor.h"
#include "LinuxIPC.h"
#include <gtk/gtk.h>
#include <nlohmann/json.hpp>
#include <poll.h>
#include <iostream>
using Json=nlohmann::json;
struct Helper {
  LinuxIPC ipc{3};
  LinuxEditorState state;
  std::unique_ptr<LinuxEditorView> view;
  std::string previousError;
  void message(const std::string& text){
    const auto data=Json::parse(text,nullptr,false);if(!data.is_object())return;
    auto op=data.value("op",std::string());
    if(op=="boot"){
      state.resources=data.at("resources");state.initial=data.at("state");state.width=data.at("width");state.height=data.at("height");
      state.send=[this](const std::string& text){ipc.send(text);};view=std::make_unique<LinuxEditorView>(state);
      view->parent(reinterpret_cast<void*>(data.at("parent").get<uintptr_t>()));view->show(data.value("visible",false));
    }else if(view){
      if(op=="messages"){auto messages=data.at("messages").get<std::vector<std::string>>();state.messages.insert(state.messages.end(),messages.begin(),messages.end());}
      else if(op=="parent")view->parent(reinterpret_cast<void*>(data.at("parent").get<uintptr_t>()));
      else if(op=="size")view->size(data.at("width"),data.at("height"));
      else if(op=="show")view->show(data.at("visible"));
      else if(op=="restore"){state.initial=data.at("state");view->restore();}
    }
  }
  gboolean poll(){
    try{
      for(auto& text:ipc.receive())message(text);
      if(view)view->poll();
      if(state.lastError!=previousError){previousError=state.lastError;ipc.send(Json{{"kind","diagnostic"},{"text",previousError}}.dump());}
    }catch(const std::exception& e){ipc.send(Json{{"kind","diagnostic"},{"text",e.what()}}.dump());ipc.flush();}
    if(!ipc.connected){gtk_main_quit();return G_SOURCE_REMOVE;}
    return G_SOURCE_CONTINUE;
  }
};
int main(){
  // GTK is loaded only here, never into a GTK2/GTK4 DAW process.
  g_setenv("GDK_BACKEND","x11",TRUE);
  if(!gtk_init_check(nullptr,nullptr))return 1;
  Helper helper;auto timer=g_timeout_add(16,[](gpointer data)->gboolean{return static_cast<Helper*>(data)->poll();},&helper);
  gtk_main();if(helper.ipc.connected)g_source_remove(timer);return 0;
}
