// Silent regression for host-buffer independence and concurrent native instances.
#include "Runtime.h"
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <thread>
#include <vector>
using Json=nlohmann::json;
static void require(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
static void control(Runtime& runtime,int id,int value){runtime.message(Json{{"type","control"},{"timbre",0},{"parameter",id},{"value",value}}.dump());}
static void configure(Runtime& runtime,int note,bool poly=false){
  for(auto p:{std::pair{17,127},{18,0},{19,0},{9,127},{3,0},{4,127},{5,127},{1,80},{2,45},{189,9}})control(runtime,p.first,p.second);
  Json target={{"kind","synth"},{"timbre",0},{"parameter",1}},values=Json::array(),knobs=Json::array();
  for(unsigned i=0;i<128;i++)values.push_back(int(i%3)*20-20);
  for(unsigned i=0;i<8;i++)knobs.push_back({{"value",0},{"bindings",Json::array()}});
  Json lane={{"id","stream"},{"enabled",true},{"target",target},{"amount",100},{"length",3},{"resolution","1/48"},{"motion","Slide"},{"direction","Forward"},{"runMode","Loop"},{"values",values}};
  if(poly){lane["playback"]="Poly";control(runtime,89,3000);}
  Json volume=lane;volume["id"]="stream-volume";volume["playback"]="Timbre";volume["target"]={{"kind","volume"}};volume["amount"]=30;volume["motion"]="Step";
  Json macro=lane;macro["id"]="stream-macro";macro["playback"]="Timbre";macro["target"]={{"kind","macro"},{"macro",0}};macro["values"]=Json::array();for(unsigned i=0;i<128;i++)macro["values"].push_back(20+int(i%3)*20);
  knobs[0]["value"]=25;knobs[0]["bindings"]=Json::array({{{"target",{{"kind","volume"}}},{"amount",-50}}});
  runtime.message(Json{{"type","modulation"},{"state",{{"config",{{"version",1},{"tracks",Json::array({Json::array({lane,volume,macro}),Json::array(),Json::array(),Json::array()})}}},{"macros",{{"version",1},{"knobs",knobs},{"bases",Json::array({{{"target",{{"kind","volume"}}},{"value",42.5}}})}}},{"targets",Json::array({{{"target",target},{"min",0},{"max",127},{"base",80},{"available",true}},{{"target",{{"kind","volume"}}},{"min",0},{"max",100},{"base",30},{"available",true}}})}}}}.dump());
  runtime.midi(0x90,note,100);
}
static std::vector<float> timeline(Runtime& runtime,unsigned block,int note,bool record=false){
  constexpr unsigned total=24000;std::array<float,512> l{},r{};std::vector<float> pcm;pcm.reserve(total*2);
  if(record)runtime.message(R"({"type":"record-start","id":"stream-check"})");
  for(unsigned at=0;at<total;){
    if(at==1031)runtime.midi(0x90,note+7,80);
    if(at==4099)control(runtime,1,60);
    if(at==8127)runtime.parameter(0,50);
    if(at==17333)runtime.midi(0x80,note,0);
    unsigned end=total;for(unsigned event:{1031u,4099u,8127u,17333u})if(event>at)end=std::min(end,event);
    auto n=std::min(block,end-at);runtime.render(l.data(),r.data(),n);
    for(unsigned i=0;i<n;i++){require(std::isfinite(l[i])&&std::isfinite(r[i]),"Finite streaming PCM");pcm.push_back(l[i]);pcm.push_back(r[i]);}at+=n;
  }
  if(record)runtime.message(R"({"type":"record-stop","id":"stream-check"})");
  return pcm;
}
static void exact(const std::vector<float>& a,const std::vector<float>& b){require(a.size()==b.size(),"PCM lengths agree");for(size_t i=0;i<a.size();i++)require(std::bit_cast<uint32_t>(a[i])==std::bit_cast<uint32_t>(b[i]),"Host block sizes / other instances cannot change PCM");}
int main(int argc,char** argv){try{
  require(argc==2,"Resource path required");
  Runtime held(argv[1],48000);configure(held,60);std::array<float,32> heldL{},heldR{};held.render(heldL.data(),heldR.data(),32);
  auto before=Json::parse(held.stats())["modulation"];held.midi(0x90,72,90);held.render(heldL.data(),heldR.data(),32);auto after=Json::parse(held.stats())["modulation"];
  require(before["positions"]==after["positions"]&&before["values"]==after["values"],"MIDI preserves held Step/Slide and macro offsets inside a partial quantum");
  held.parameter(0,50);held.render(heldL.data(),heldR.data(),32);after=Json::parse(held.stats())["modulation"];
  require(before["positions"]==after["positions"]&&before["values"]!=after["values"],"Macro edits rebase held motion without advancing its clock");
  control(held,1,90);held.render(heldL.data(),heldR.data(),16);after=Json::parse(held.stats())["modulation"];
  bool rebased=false;for(auto& spec:after["values"])if(spec["target"].value("kind","")=="synth"&&spec["target"]["parameter"]==1)rebased=spec["value"]==70;
  require(rebased&&before["positions"]==after["positions"],"Scalar edits retain the held offset and the new base inside a partial quantum");
  Runtime poly(argv[1],48000);configure(poly,60,true);std::array<float,512> polyL{},polyR{};poly.render(polyL.data(),polyR.data(),512);poly.render(polyL.data(),polyR.data(),272);
  require(Json::parse(poly.stats())["modulation"]["positions"][0][0]==0,"The synthesis pool must not pre-render Poly motion past the requested audio frames");
  for(double rate:{44100.,48000.,96000.}){
    Runtime baseline(argv[1],rate);configure(baseline,60);auto expected=timeline(baseline,128,60);
    for(unsigned size:{1u,17u,32u,64u,257u,512u}){Runtime split(argv[1],rate);configure(split,60);split.telemetry(false);exact(expected,timeline(split,size,60));}
    Runtime budget(argv[1],rate);unsigned total=0;std::array<float,512> l{},r{};
    for(unsigned size:{1u,17u,32u,64u,257u,512u,33u}){
      budget.render(l.data(),r.data(),size);total+=size;
      require(Json::parse(budget.stats())["frames"]==uint64_t(std::floor(total*48000./rate)),"Short host buffers never pre-render a whole 128-frame block");
    }
    budget.takeMessages();unsigned updates=0;
    for(unsigned at=0;at<unsigned(rate);){auto n=std::min(32u,unsigned(rate)-at);budget.render(l.data(),r.data(),n);at+=n;for(auto& text:budget.takeMessages())if(Json::parse(text)["type"]=="stats")updates++;}
    require(updates>=19&&updates<=21,"Telemetry is limited to 20 Hz independently of host block size");
    budget.telemetry(false);for(int i=0;i<100;i++)budget.render(l.data(),r.data(),32);
    for(auto& text:budget.takeMessages())require(Json::parse(text)["type"]!="stats","Closed editors generate no periodic telemetry");
    budget.telemetry(true);for(int i=0;i<100;i++)budget.render(l.data(),r.data(),128);
    bool resumed=false;for(auto& text:budget.takeMessages())resumed|=Json::parse(text)["type"]=="stats";require(resumed,"Reopened editors resume telemetry");
    Runtime recorder(argv[1],rate);configure(recorder,60);auto pcm=timeline(recorder,32,60,true);std::vector<int> captured;
    for(auto& text:recorder.takeMessages()){auto data=Json::parse(text);if(data["type"]=="record-chunk")for(auto& value:data["pcm"])captured.push_back(value.get<int>());}
    require(captured.size()==pcm.size(),"Recording retains every output frame with short host blocks");
    for(size_t i=0;i<pcm.size();i++){double x=std::clamp(double(pcm[i]),-1.,1.);require(captured[i]==int(std::floor(x*(x<0?32768:32767)+.5)),"Recording captures the actual host output");}
    std::cout<<rate<<" Hz: exact PCM at 1/17/32/64/128/257/512 frames; clocks, telemetry and recording passed\n";
  }
  // Contexts are initialized together, then migrate to independent audio threads.
  std::array<std::unique_ptr<Runtime>,4> instances;std::array<std::vector<float>,4> expected,actual;
  for(unsigned i=0;i<instances.size();i++){
    Runtime isolated(argv[1],48000);configure(isolated,48+i*5);expected[i]=timeline(isolated,128,48+i*5);
    instances[i]=std::make_unique<Runtime>(argv[1],48000);configure(*instances[i],48+i*5);instances[i]->telemetry(false);
  }
  std::array<std::exception_ptr,4> errors;std::vector<std::thread> workers;
  for(unsigned i=0;i<instances.size();i++)workers.emplace_back([&,i]{try{actual[i]=timeline(*instances[i],std::array{17u,32u,64u,257u}[i],48+i*5);}catch(...){errors[i]=std::current_exception();}});
  for(auto& worker:workers)worker.join();for(unsigned i=0;i<instances.size();i++){if(errors[i])std::rethrow_exception(errors[i]);exact(expected[i],actual[i]);}
  std::cout<<"Four concurrent plugin instances preserve isolated PCM and state. No audio device opened.\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
