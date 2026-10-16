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
      std::exception_ptr threadError;std::thread audioThread([&]{try{
        a.render(l,r,257);a.message("{\"type\":\"stop\"}");
        Runtime fresh(argv[1],rate);fresh.midi(0x90,60,100);fresh.render(l,r,257);fresh.message("{\"type\":\"stop\"}");
      }catch(...){threadError=std::current_exception();}});audioThread.join();if(threadError)std::rethrow_exception(threadError);
      a.midi(0x80,69,0);a.message("{\"type\":\"stop\"}");
      // Imported browser patterns must neither emit notes on DAW Play nor
      // intercept incoming MIDI. Modulation remains native and follows PPQ.
      Runtime daw(argv[1],rate);Json track={{"enabled",true},{"length",16},{"resolution","1/16"},{"keyTriggered",true},{"steps",Json::array()}};
      for(unsigned i=0;i<128;i++)track["steps"].push_back({{"notes",Json::array({60,64,67})},{"velocity",100},{"gate",75}});
      daw.message(Json{{"type","sequencer"},{"config",{{"version",1},{"tracks",Json::array({track,track,track,track})}}}}.dump());
      daw.message(R"({"type":"sequence-play"})");daw.message(R"({"type":"transport","playing":true,"tempo":120,"beat":2,"seek":true})");
      for(int i=0;i<10;i++)daw.render(l,r,257);
      require(Json::parse(daw.stats())["voices"]==0&&std::all_of(l,l+257,[](float v){return v==0;}),"DAW Play cannot generate internal pattern notes");
      const Json cutoff={{"kind","synth"},{"timbre",0},{"parameter",1}};
      Json steps=Json::array();for(unsigned i=0;i<128;i++)steps.push_back(i%3==0?-30:i%3==1?0:30);
      Json lane={{"id","native-clock"},{"enabled",true},{"target",cutoff},{"amount",100},{"length",3},{"resolution","1/16"},{"motion","Step"},{"direction","Forward"},{"runMode","Loop"},{"values",steps}};
      for(auto& knob:knobs)knob["bindings"]=Json::array();
      daw.message(Json{{"type","modulation"},{"state",{{"config",{{"version",1},{"tracks",Json::array({Json::array({lane}),Json::array(),Json::array(),Json::array()})}}},{"macros",{{"version",1},{"knobs",knobs},{"bases",Json::array()}}},{"targets",Json::array({{{"target",cutoff},{"min",0},{"max",127},{"base",80},{"available",true}}})}}}}.dump());
      daw.message(R"({"type":"control","timbre":0,"parameter":1,"value":80})");
      daw.message(R"({"type":"transport","playing":true,"tempo":120,"beat":2,"seek":true})");daw.render(l,r,257);
      auto status=Json::parse(daw.stats())["modulation"];
      require(status["backend"]=="native-rust"&&status["positions"][0][0]==2&&status["values"][0]["value"]==110,"Rust modulation starts at DAW PPQ position");
      daw.message(R"({"type":"transport","playing":true,"tempo":95.25,"beat":0.25,"seek":true})");daw.render(l,r,257);
      require(Json::parse(daw.stats())["modulation"]["positions"][0][0]==1,"DAW seek and fractional tempo reach native modulation");
      daw.midi(0x90,69,100);for(int i=0;i<4;i++)daw.render(l,r,257);
      require(Json::parse(daw.stats())["voices"]==1,"DAW MIDI bypasses an imported key-triggered note pattern");
      daw.message(R"({"type":"stop"})");daw.render(l,r,257);status=Json::parse(daw.stats())["modulation"];
      require(status["running"]==false&&status["values"].empty(),"Host Stop restores temporary modulation and stops its Rust clock");
      require(a.stats().find("\"backend\":\"native-rust\"")!=std::string::npos,"AOT native Rust backend");
      require(a.stats().find("\"capacity\":128")!=std::string::npos,"Same expanded 128-voice engine");
      Runtime controls(argv[1],rate);controls.takeMessages();
      controls.message(R"({"type":"control","timbre":0,"parameter":116,"value":1})");
      for(int i=0;i<64;i++){controls.midi(0xb0,1,i);controls.midi(0xe0,0,96);}
      require(controls.takeMessages().empty(),"MIDI bursts never serialize full state per event");
      int refreshes=0;Json latest;
      for(int i=0;i<40;i++){
        controls.render(l,r,128);
        for(const auto& text:controls.takeMessages()){auto message=Json::parse(text);if(message["type"]=="state"){refreshes++;latest=message;}}
      }
      require(refreshes==1&&latest["program"]["timbres"][0][138]==63&&latest["program"]["timbres"][0][137]==4096,"Coalesced UI state retains the last wheel and pitch values");
      require(!latest["program"].contains("effects"),"MIDI refresh does not rebuild effect controls");
      controls.midi(0xb0,7,41);controls.midi(0xb1,7,73);controls.message(R"({"type":"stop"})");
      bool final=false;for(const auto& text:controls.takeMessages()){auto message=Json::parse(text);if(message["type"]=="state"){require(message["midiBatch"].size()==2,"Coalescing preserves CC7 on different channels");final=true;}}
      require(final,"Stopping flushes pending controller values for saving");
      controls.midi(0xb0,1,99);controls.message(Json{{"type","load"},{"program",latest["program"]}}.dump());
      controls.takeMessages();for(int i=0;i<20;i++)controls.render(l,r,128);
      for(const auto& text:controls.takeMessages())require(Json::parse(text)["type"]!="state","Loading a program clears obsolete MIDI UI updates");
      std::cout<<"Native Rust DSP / shared controls: "<<rate<<" Hz, MIDI, variable blocks and instance isolation passed; peak "<<peak<<"\n";
    }
  }catch(const std::exception& e){std::cerr<<e.what()<<"\n";return 1;}
}
