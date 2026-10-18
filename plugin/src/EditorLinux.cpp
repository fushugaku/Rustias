#include "Editor.h"
#include "Instrument.h"
#include "LinuxIPC.h"
#include <nlohmann/json.hpp>
#include <spawn.h>
#include <sys/wait.h>
#include <signal.h>
#include <chrono>
#include <thread>
#include <filesystem>
#include <cstring>
extern char** environ;
using Json=nlohmann::json;
struct Editor::Impl {
  Instrument& instrument;
  std::unique_ptr<LinuxIPC> channel;
  pid_t child=0;
  unsigned width,height;
  bool visible=false;
  explicit Impl(Instrument& source):instrument(source),width(source.width),height(source.height){}
  ~Impl(){
    channel.reset();if(!child)return;
    const auto deadline=std::chrono::steady_clock::now()+std::chrono::milliseconds(500);
    while(std::chrono::steady_clock::now()<deadline){if(waitpid(child,nullptr,WNOHANG)==child)return;std::this_thread::sleep_for(std::chrono::milliseconds(5));}
    kill(child,SIGTERM);while(waitpid(child,nullptr,0)<0&&errno==EINTR){}
  }
  void send(Json data){if(channel)channel->send(data.dump());}
  bool start(uintptr_t parent){
    int sockets[2];if(socketpair(AF_UNIX,SOCK_STREAM|SOCK_CLOEXEC,0,sockets))return false;
    // Keep the source distinct from child fd 3 so dup2 clears CLOEXEC reliably.
    int helperSocket=fcntl(sockets[1],F_DUPFD_CLOEXEC,10);close(sockets[1]);
    if(helperSocket<0){close(sockets[0]);return false;}
    posix_spawn_file_actions_t actions;posix_spawn_file_actions_init(&actions);
    posix_spawn_file_actions_addclose(&actions,sockets[0]);posix_spawn_file_actions_adddup2(&actions,helperSocket,3);posix_spawn_file_actions_addclose(&actions,helperSocket);
    auto executable=(std::filesystem::path(instrument.resources)/"editor-helper").string();char* arguments[]={executable.data(),nullptr};
    const auto status=posix_spawn(&child,executable.c_str(),&actions,nullptr,arguments,environ);
    posix_spawn_file_actions_destroy(&actions);close(helperSocket);
    if(status){close(sockets[0]);child=0;instrument.lastError="Cannot start Linux editor: "+std::string(strerror(status));return false;}
    channel=std::make_unique<LinuxIPC>(sockets[0]);
    send({{"op","boot"},{"resources",instrument.resources},{"state",instrument.initialState()},{"parent",parent},{"width",width},{"height",height},{"visible",visible}});return true;
  }
};
Editor::Editor(Instrument& source):impl(std::make_unique<Impl>(source)){}
Editor::~Editor()=default;
bool Editor::parent(void* parent){auto id=reinterpret_cast<uintptr_t>(parent);if(!id)return false;if(!impl->channel)return impl->start(id);impl->send({{"op","parent"},{"parent",id}});return true;}
void Editor::size(unsigned width,unsigned height){impl->width=width;impl->height=height;impl->send({{"op","size"},{"width",width},{"height",height}});}
void Editor::show(bool visible){impl->visible=visible;impl->send({{"op","show"},{"visible",visible}});}
void Editor::poll(){
  if(!impl->channel)return;
  for(auto& message:impl->channel->receive()){
    auto data=Json::parse(message,nullptr,false);if(!data.is_object())continue;
    if(data.value("kind",std::string())=="diagnostic")impl->instrument.lastError=data.value("text",std::string());
    else impl->instrument.uiMessage(message);
  }
  if(!impl->channel->connected){impl->instrument.lastError="Linux editor closed. Check that WebKitGTK 4.1 and an X11/XWayland display are available.";return;}
  auto messages=impl->instrument.pull();if(!messages.empty())impl->send({{"op","messages"},{"messages",std::move(messages)}});
}
void Editor::restore(){impl->send({{"op","restore"},{"state",impl->instrument.initialState()}});}
