#include "Runtime.h"
#include <cmath>
#include <iostream>
#include <stdexcept>
#include <vector>
#include <algorithm>
#include <nlohmann/json.hpp>
#include <thread>
static void require(bool b,const char* m){if(!b)throw std::runtime_error(m);}
int main(int argc,char** argv){
  try{
    require(argc==2,"Resource directory required");
    for(double rate:{48000.,44100.,96000.}){
      Runtime a(argv[1],rate),b(argv[1],rate);float l[257],r[257];
      a.message("{\"type\":\"control\",\"timbre\":0,\"parameter\":9,\"value\":127}");a.midi(0x90,69,100);
      float peak=0;for(unsigned block=0;block<100;block++){a.render(l,r,257);for(auto x:l){require(std::isfinite(x),"Finite audio");peak=std::max(peak,std::abs(x));}}
      if(peak<=.001f){std::cerr<<a.stats()<<"\n";for(auto& message:a.takeMessages())std::cerr<<message<<"\n";}
      require(peak>.001f,"Host MIDI produces stereo audio");
      using Json=nlohmann::json;const Json target={{"kind","synth"},{"timbre",0},{"parameter",7}};Json knobs=Json::array();
      for(unsigned i=0;i<8;i++)knobs.push_back({{"name","Macro "+std::to_string(i+1)},{"value",0},{"bindings",i?Json::array():Json::array({{{"target",target},{"amount",100}}})}});
      a.message(Json{{"type","modulation"},{"state",{{"config",{{"version",1},{"tracks",Json::array({Json::array(),Json::array(),Json::array(),Json::array()})}}},{"macros",{{"version",1},{"knobs",knobs},{"bases",Json::array({{{"target",target},{"value",0},{"label","Level"}}})}}},{"targets",Json::array({{{"target",target},{"min",0},{"max",127},{"base",0},{"available",true}}})},{"effects",{{"slots",Json::array()}}},{"circuits",Json::array()},{"engine",Json::object()}}}}.dump());
      a.parameter(0,100);a.render(l,r,257);for(int i=0;i<4;i++)a.render(l,r,257);require(std::any_of(l,l+257,[](float v){return std::abs(v)>.001f;}),"DAW macro reaches a bound sound parameter");
      a.parameter(0,0);for(int i=0;i<40;i++)a.render(l,r,257);require(std::all_of(l,l+257,[](float v){return std::abs(v)<.00001f;}),"DAW macro can close its bound level");
      b.render(l,r,257);require(std::all_of(l,l+257,[](float v){return v==0;}),"Plugin instances are isolated");
      std::exception_ptr threadError;std::thread audioThread([&]{try{a.render(l,r,257);a.message("{\"type\":\"stop\"}");}catch(...){threadError=std::current_exception();}});audioThread.join();if(threadError)std::rethrow_exception(threadError);
      a.midi(0x80,69,0);a.message("{\"type\":\"stop\"}");
      require(a.stats().find("\"backend\":\"native-rust\"")!=std::string::npos,"AOT native Rust backend");
      require(a.stats().find("\"capacity\":128")!=std::string::npos,"Same expanded 128-voice engine");
      std::cout<<"Native Rust DSP / shared controls: "<<rate<<" Hz, MIDI, variable blocks and instance isolation passed; peak "<<peak<<"\n";
    }
  }catch(const std::exception& e){std::cerr<<e.what()<<"\n";return 1;}
}
