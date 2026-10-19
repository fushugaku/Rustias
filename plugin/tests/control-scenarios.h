#pragma once
#include "Runtime.h"
#include <nlohmann/json.hpp>
namespace ControlScenario {
using Json=nlohmann::json;
inline void control(Runtime& r,unsigned id,int value,unsigned t=0){r.message(Json{{"type","control"},{"timbre",t},{"parameter",id},{"value",value}}.dump());}
inline Json lane(Json target,unsigned i){
  Json values=Json::array();for(unsigned s=0;s<128;s++)values.push_back(int(s%5)*9-18);
  return {{"id","control-"+std::to_string(i)},{"enabled",true},{"target",target},{"amount",70},{"length",5},{"resolution",i%2?"1/48":"1/16"},{"motion",i%2?"Slide":"Step"},{"direction","Forward"},{"runMode","Loop"},{"values",values}};
}
inline void configure(Runtime& r,unsigned scenario){
  for(auto p:{std::pair{17,127},{18,0},{19,0},{9,127},{3,0},{4,127},{5,127},{1,80},{2,45},{116,1}})control(r,p.first,p.second);
  Json effect={{"kind",20},{"enabled",true},{"master",true},{"parameters",Json::array({50,63,28,82,53,55,100,64,72,0,0,0,0,0,0,0,0,0,0,0})}};
  r.message(Json{{"type","effect"},{"slot",17},{"program",effect}}.dump());
  auto chain=effect;chain["id"]="control-chain";
  r.message(Json{{"type","effect-chain"},{"timbre",0},{"chain",Json::array({chain})}}.dump());
  Json synth={{"kind","synth"},{"timbre",0},{"parameter",1}},fx={{"kind","effect"},{"slot",17},{"effectKind",20},{"parameter",0}},extra={{"kind","chain-effect"},{"timbre",0},{"id","control-chain"},{"effectKind",20},{"parameter",0}},vol={{"kind","volume"}};
  Json targets=Json::array(),bases=Json::array();
  for(auto target:{synth,fx,extra,vol}){double base=target==synth?80:target==vol?30:50;targets.push_back({{"target",target},{"min",0},{"max",target==vol?100:127},{"base",base}});bases.push_back({{"target",target},{"value",base-10}});}
  Json knobs=Json::array();
  for(unsigned k=0;k<8;k++){
    Json bindings=Json::array();for(unsigned b=0;b<12;b++)bindings.push_back({{"target",targets[b%4]["target"]},{"amount",int((k+b)%3)*10-10}});
    knobs.push_back({{"value",25+k},{"bindings",bindings}});
  }
  Json tracks=Json::array({Json::array({lane(synth,0),lane(fx,1),lane(extra,2),lane({{"kind","macro"},{"macro",0}},3),lane(vol,4)}),Json::array(),Json::array(),Json::array()});
  if(scenario==1){targets[1]["min"]=-20;targets[1]["max"]=280;targets[1]["base"]=5;bases[1]["value"]=5;}
  if(scenario==2){targets[2]["available"]=false;tracks[0][0]["playback"]="Poly";}
  if(scenario==3){auto duplicate=chain;duplicate["kind"]=1;duplicate["parameters"]=Json::array({100,0,70,19,50,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0});chain=Json::array({duplicate,chain});}
  r.message(Json{{"type","modulation"},{"state",{{"config",{{"version",1},{"tracks",tracks}}},{"macros",{{"knobs",knobs},{"bases",bases}}},{"targets",targets},{"effects",{{"slots",Json::array({nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,effect})},{"chains",Json::array({scenario==3?chain:Json::array({chain})})}}}}}}.dump());
  for(unsigned n=0;n<6;n++)r.midi(0x90,48+n*3,100);
}
}
