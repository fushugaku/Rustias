// Both executables use identical Rust DSP. The oracle uses the former shared JS
// worklet; the shipped adapter calls native Rust directly. No device is opened.
#include "Runtime.h"
#include <nlohmann/json.hpp>
#include <array>
#include <bit>
#include <cmath>
#include <fstream>
#include <iostream>
#include <stdexcept>
using Json=nlohmann::json;
static std::array<Json,31> effects;
static void control(Runtime& r,unsigned id,int value,unsigned t=0){r.message(Json{{"type","control"},{"timbre",t},{"parameter",id},{"value",value}}.dump());}
static void motion(Runtime& r){
  Json target={{"kind","synth"},{"timbre",0},{"parameter",1}},knobs=Json::array(),values=Json::array();
  for(unsigned i=0;i<8;i++)knobs.push_back({{"value",25},{"bindings",i?Json::array():Json::array({{{"target",{{"kind","volume"}}},{"amount",-50}}})}});
  for(unsigned i=0;i<128;i++)values.push_back(int(i%3)*20-20);
  Json lane={{"id","adapter-motion"},{"enabled",true},{"target",target},{"amount",100},{"length",3},{"resolution","1/48"},{"motion","Slide"},{"direction","Forward"},{"runMode","Loop"},{"values",values}};
  auto volume=lane;volume["id"]="adapter-volume";volume["target"]={{"kind","volume"}};volume["motion"]="Step";
  auto macro=lane;macro["id"]="adapter-macro";macro["target"]={{"kind","macro"},{"macro",0}};
  r.message(Json{{"type","modulation"},{"state",{{"config",{{"version",1},{"tracks",Json::array({Json::array({lane,volume,macro}),Json::array(),Json::array(),Json::array()})}}},{"macros",{{"knobs",knobs},{"bases",Json::array({{{"target",{{"kind","volume"}}},{"value",42.5}}})}}},{"targets",Json::array({{{"target",target},{"min",0},{"max",127},{"base",80}},{{"target",{{"kind","volume"}}},{"min",0},{"max",100},{"base",30}}})}}}}.dump());
}
static void configure(Runtime& r,unsigned scene){
  for(auto p:{std::pair{17,100},{18,0},{19,0},{187,0},{9,127},{3,0},{4,127},{5,127},{1,80},{2,60},{116,1}})control(r,p.first,p.second);
  if(scene<18){control(r,189,scene);control(r,191,73);control(r,193,51);}
  else if(scene<26){control(r,17,0);control(r,19,127);control(r,171,(scene-18)%6);control(r,172,(scene-18)%4);control(r,173,29);control(r,174,83);control(r,175,57);control(r,176,99);}
  else if(scene<34){control(r,17,0);control(r,187,127);control(r,179,(scene-26)%6);control(r,180,(scene-26)%5);control(r,181,51);control(r,182,75);control(r,183,71);control(r,184,35);}
  else if(scene<64){
    // Real accepted browser presets, not arbitrary out-of-range enum values.
    Json program=effects.at(scene-33);
    r.message(Json{{"type","effect"},{"slot",17},{"program",program}}.dump());
    program["id"]="adapter-chain";r.message(Json{{"type","effect-chain"},{"timbre",0},{"chain",Json::array({program})}}.dump());
  }else if(scene==64){
    // Stress scene based on observed Logic controls: four sources, Noise, VPM, Ring and two
    // resonant MS-20 LP filters. Do not substitute an inexpensive saw-only test.
    for(auto p:{std::pair{10,3},{11,29},{12,39},{13,1},{14,1},{15,76},{18,127},{19,127},{171,4},{173,22},{174,72},{179,1},{183,76},{185,1},{186,3},{187,127},{189,2},{190,2},{1,116},{2,113},{22,43},{23,116},{25,26},{27,31},{192,22}})control(r,p.first,p.second);
  }else if(scene==65)motion(r);
  else if(scene==66){
    Json pcm=Json::array();for(unsigned i=0;i<4000;i++)pcm.push_back(float(.2*std::sin(i*.071)));
    r.message(Json{{"type","library-sample"},{"asset",1},{"data",pcm}}.dump());
    // The sample profile gets a complete synth schema.
    r.message(R"({"type":"control","timbre":0,"parameter":999,"value":0})");
    Json values;for(auto& text:r.takeMessages()){auto d=Json::parse(text);if(d["type"]=="state")values=d["program"]["timbres"][0];}
    if(!values.is_array())throw std::runtime_error("Sample controls available");
    r.message(Json{{"type","library-profile"},{"id",1},{"asset",1},{"timbre",0},{"mode",1},{"source","user:adapter"},{"values",values}}.dump());
    r.message(R"({"type":"note","timbre":0,"note":"user:adapter","velocity":100})");
  }else if(scene==67){for(unsigned t=1;t<8;t++){control(r,71,1,t);control(r,72,t,t);r.midi(0x90|t,48+t,100);}}
  else if(scene==68)r.message(R"({"type":"note","timbre":0,"note":60,"velocity":100})");
  else if(scene==69){
    control(r,119,62);
    r.message(R"({"type":"audition","timbre":0,"step":{"notes":[67,60,67],"samples":[],"velocity":100,"gate":25},"resolution":"1/32"})");
  }
}
int main(int argc,char** argv){try{
  if(argc!=5)throw std::runtime_error("Usage: adapter-check RESOURCES BROWSER_FIXTURES PCM_FILE write|compare");const bool writing=std::string(argv[4])=="write";
  {
    std::ifstream input(argv[2]);auto fixtures=Json::parse(input);
    for(auto& scenario:fixtures.at("cases")){
      Json staged;
      for(auto& op:scenario.at("operations")){
        if(op.contains("write")){const auto bytes=op["write"].get<std::vector<uint8_t>>();staged=Json::parse(bytes,nullptr,false);}
        if(op.value("call",std::string())=="rustias_effect"&&staged.is_object()&&staged.value("master",false)){
          const auto kind=staged.value("kind",0u);if(kind>0&&kind<effects.size()&&effects[kind].is_null())effects[kind]=staged;
        }
      }
    }
    for(unsigned kind=1;kind<effects.size();kind++)if(effects[kind].is_null())throw std::runtime_error("Browser effect fixture missing");
  }
  std::fstream file(argv[3],std::ios::binary|(writing?std::ios::out|std::ios::trunc:std::ios::in));if(!file)throw std::runtime_error("Open PCM reference");
  uint64_t compared=0;unsigned cases=0;
  for(double rate:{44100.,48000.,96000.})for(unsigned scene=0;scene<70;scene++){
    Runtime r(argv[1],rate);configure(r,scene);
    for(auto& text:r.takeMessages()){auto d=Json::parse(text);if(d["type"]=="warning"||d["type"]=="error")throw std::runtime_error("Adapter scene "+std::to_string(scene)+": "+text);}
    r.telemetry(false);r.midi(0x90,60,100);
    std::array<float,257> l{},rr{};
    for(unsigned at=0,block=0;at<8192;block++){
      if(at==103)r.midi(0x90,67,85);
      if(at==409){control(r,1,67);r.midi(0xb0,1,71);r.midi(0xe0,0,80);}
      if(at==2001){r.parameter(0,50);r.parameter(8,42.5);}
      if(at==5003)r.midi(0x80,60,0);
      unsigned end=8192;for(unsigned event:{103u,409u,2001u,5003u})if(event>at)end=std::min(end,event);
      auto n=std::min(end-at,std::array{1u,17u,32u,64u,128u,257u}[block%6]);r.render(l.data(),rr.data(),n);
      for(unsigned i=0;i<n;i++)for(float v:{l[i],rr[i]}){
        if(!std::isfinite(v))throw std::runtime_error("Finite native adapter PCM");const auto bits=std::bit_cast<uint32_t>(v);
        if(writing)file.write(reinterpret_cast<const char*>(&bits),4);else{uint32_t before=0;file.read(reinterpret_cast<char*>(&before),4);if(!file||before!=bits)throw std::runtime_error("Native adapter differs: rate="+std::to_string(rate)+", scene="+std::to_string(scene)+", frame="+std::to_string(at+i));}
        compared++;
      }
      at+=n;
    }
    cases++;
  }
  if(!writing&&file.peek()!=std::char_traits<char>::eof())throw std::runtime_error("Complete PCM reference consumed");
  std::cout<<cases<<" native adapter scenes, "<<compared/2<<" stereo frames "<<(writing?"saved from the shared-worklet oracle":"match the shared-worklet oracle bit for bit")<<". No audio device opened.\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
