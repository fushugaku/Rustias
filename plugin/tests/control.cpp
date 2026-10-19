// Offline regression/benchmark for the native control boundary. No audio device.
// Optional write/compare modes also verify releases against a frozen old engine.
#include "control-scenarios.h"
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <bit>
#include <chrono>
#include <cmath>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <thread>
using Json=nlohmann::json;
static void require(bool ok,const std::string& what){if(!ok)throw std::runtime_error(what);}
using namespace ControlScenario;
static Json run(const char* resources,double rate,unsigned scenario,std::ostream* pcm){
  Runtime r(resources,rate);configure(r,scenario);r.takeMessages();
  r.message(R"({"type":"record-start","id":"control-recording"})");
  Json states=Json::array();std::vector<int> recorded;uint64_t recordingFrames=0;unsigned chunks=0,stats=0;
  auto consume=[&]{for(auto& text:r.takeMessages()){
    auto message=Json::parse(text);auto type=message.value("type",std::string());
    require(type!="warning"&&type!="error","Control boundary rejected a valid transaction: "+text);
    if(type=="state")states.push_back(message);
    if(type=="stats")stats++;
    if(type=="record-chunk"){require(message["index"]==chunks++,"Recording chunk order");recordingFrames=message["frames"];for(auto& v:message["pcm"])recorded.push_back(v.get<int>());}
  }};
  std::array<float,257> l{},rr{};std::vector<int> expected;expected.reserve(96000);
  constexpr unsigned total=48000;
  for(unsigned at=0,block=0;at<total;block++){
    if(at==409){r.midi(0xb0,7,74);r.midi(0xb1,7,39);r.midi(0xe0,0,80);}
    if(at==2001){r.parameter(0,50);r.parameter(7,33);control(r,1,90);}
    if(at==8127){r.telemetry(false);r.midi(0xb0,1,63);}
    if(at==16385){r.telemetry(true);r.midi(0xe0,0,40);}
    if(at==24001){r.message(R"({"type":"transport","playing":true,"tempo":137,"beat":3.25,"seek":true})");}
    if(at==33001){r.message(R"({"type":"stop"})");r.midi(0x90,60,90);}
    unsigned end=total;for(unsigned event:{409u,2001u,8127u,16385u,24001u,33001u})if(event>at)end=std::min(end,event);
    auto n=std::min(end-at,std::array{1u,17u,32u,64u,128u,257u}[block%6]);r.render(l.data(),rr.data(),n);
    for(unsigned i=0;i<n;i++)for(float v:{l[i],rr[i]}){
      require(std::isfinite(v),"Finite control PCM");if(pcm)pcm->write(reinterpret_cast<const char*>(&v),sizeof v);
      double x=std::clamp(double(v),-1.,1.);expected.push_back(int(std::floor(x*(x<0?32768:32767)+.5)));
    }
    at+=n;if(block%7==0)consume();
  }
  r.message(R"({"type":"record-stop","id":"control-recording"})");
  r.message(R"({"type":"stop"})");consume();
  require(recordingFrames==total&&recorded==expected,"Pooled recording preserves every quantized host frame");
  double expectedStats=(total-(16385-8127))*20/rate;require(states.size()>=3&&stats+1>=std::floor(expectedStats)&&stats<=std::ceil(expectedStats)+1,"Snapshots/telemetry: states="+std::to_string(states.size())+", stats="+std::to_string(stats)+", rate="+std::to_string(rate));
  // Explicit readback must contain restored bases and complete effects.
  r.message(R"({"type":"control","timbre":0,"parameter":999,"value":0})");consume();
  require(states.back()["program"].contains("effects"),"Full snapshots retain fixed and chain effects");
  return states;
}
static void pressure(const char* resources){
  Runtime r(resources,48000);r.takeMessages();
  for(unsigned i=0;i<1000;i++)r.message(R"({"type":"control","timbre":0,"parameter":999,"value":0})");
  r.message(R"({"type":"effect","slot":17,"program":{"kind":20,"enabled":true,"master":true,"parameters":[50,63,28,82,53,55,100,64,72,0,0,0,0,0,0,0,0,0,0,0]}})");
  r.message(R"({"type":"control","timbre":0,"parameter":999,"value":0})");
  r.midi(0xb0,7,111);r.midi(0xb1,7,73);r.message(R"({"type":"stop"})");
  // No render after Stop: final MIDI must be delivered by the worker alone.
  bool final=false,correctedEffects=false;
  auto deadline=std::chrono::steady_clock::now()+std::chrono::seconds(2);
  while((!final||!correctedEffects)&&std::chrono::steady_clock::now()<deadline){
   for(auto& text:r.takeRealtimeMessages()){
    auto d=Json::parse(text);
    if(d.value("type",std::string())=="state"&&d["program"].contains("effects")&&d["program"]["effects"]["slots"][17]["kind"]==20)correctedEffects=true;
    if(d.value("type",std::string())=="state"&&d.contains("midi")&&d["midi"]==Json::array({177,7,73})){
      final=true;require(d["midiBatch"]==Json::array({Json::array({176,7,111}),Json::array({177,7,73})}),"Pressure preserves final per-channel CC7 batch");
    }
  }
   if(!final||!correctedEffects)std::this_thread::sleep_for(std::chrono::milliseconds(1));
  }
  require(final,"Stop delivers the final state even when snapshot slots are exhausted");
  require(correctedEffects,"A final MIDI state cannot replace a pending full effects correction");
}
static void telemetryState(const char* resources){
  Runtime r(resources,48000);configure(r,0);r.takeMessages();
  std::array<float,120> l{},right{};
  for(unsigned i=0;i<20;i++)r.render(l.data(),right.data(),120);
  Json captured;
  for(auto& text:r.takeMessages()){auto d=Json::parse(text);if(d.value("type",std::string())=="stats")captured=d;}
  require(captured.is_object(),"The typed telemetry snapshot was delivered");
  for(auto key:{"type","audibleFrames","peak","callbacks","sequence"})captured.erase(key);
  require(captured==Json::parse(r.stats()),"Typed telemetry preserves the independent synchronous status schema and values");
}
int main(int argc,char** argv){try{
  require(argc==2||argc==4,"Usage: control-check RESOURCES [REFERENCE_PREFIX write|compare]");
  Json states=Json::array();std::ofstream pcm;std::string prefix=argc==4?argv[2]:"",mode=argc==4?argv[3]:"";
  if(argc==4)pcm.open(prefix+(mode=="write"?".pcm":".actual.pcm"),std::ios::binary);
  for(double rate:{44100.,48000.,96000.})for(unsigned scenario=0;scenario<4;scenario++)states.push_back(run(argv[1],rate,scenario,argc==4?&pcm:nullptr));
  pcm.close();pressure(argv[1]);telemetryState(argv[1]);
  if(mode=="write"){std::ofstream file(prefix+".json");file<<states.dump();}
  if(mode=="compare"){
    std::ifstream file(prefix+".json");auto before=Json::parse(file);require(before==states,"Native snapshots/state transactions differ from frozen release");
    std::ifstream oldPCM(prefix+".pcm",std::ios::binary),newPCM(prefix+".actual.pcm",std::ios::binary);
    std::vector<char> a((std::istreambuf_iterator<char>(oldPCM)),{}),b((std::istreambuf_iterator<char>(newPCM)),{});require(a==b,"Control PCM differs from frozen release");
  }
  std::cout<<"12 control timelines, 576000 stereo frames: FX Slide, dense macros, MIDI state, Stop, telemetry and recording passed. No device opened.\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
