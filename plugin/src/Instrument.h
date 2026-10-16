#pragma once
#include <clap/clap.h>
#include "Runtime.h"
#include <array>
#include <atomic>
#include <deque>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <vector>
class Editor;
struct Instrument {
  clap_plugin_t api{};
  const clap_host_t* host;
  std::string resources;
  std::unique_ptr<Runtime> runtime;
  std::unique_ptr<Editor> editor;
  std::array<std::atomic<double>,9> parameters{};
  std::atomic<unsigned> uiParameters{0};
  std::atomic<bool> restoredParameters{false};
  std::atomic<bool> enabled{true};
  std::mutex incomingMutex,stateMutex;
  std::deque<std::string> incoming;
  std::map<std::string,std::pair<uint64_t,std::string>> remembered;
  uint64_t serial=0;
  std::string session="null",lastError;
  std::array<std::string,256> outgoing;
  std::atomic<unsigned> outgoingRead{0},outgoingWrite{0};
  double rate=48000,previousBeat=0,previousFrames=0,previousTempo=120;
  bool active=false,hostPlaying=false,haveTransport=false;
  unsigned width=1100,height=800;
  explicit Instrument(const clap_host_t*,std::string);
  ~Instrument();
  bool activate(double);
  void queue(const std::string&,bool remember=true);
  void drain();
  void publish(std::string);
  std::vector<std::string> pull();
  void uiMessage(const std::string&);
  void parameter(unsigned,double,bool fromUI);
  std::string save();
  bool restore(const std::string&);
  std::string initialState();
  void processEvents(const clap_input_events_t*,const clap_output_events_t*);
  clap_process_status process(const clap_process_t*);
};
