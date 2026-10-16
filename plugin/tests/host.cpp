// Offline host regression: never opens an audio device or plays to system output.
#include "Instrument.h"
#include <nlohmann/json.hpp>
#include <cmath>
#include <iostream>
#include <stdexcept>
#include <algorithm>
using Json=nlohmann::json;
static void require(bool b,const char* m){if(!b)throw std::runtime_error(m);}
static float render(Instrument& synth,bool note=false){
  float l[257]{},r[257]{};float* channels[]={l,r};clap_audio_buffer_t out{};out.data32=channels;out.channel_count=2;
  clap_event_midi_t midi{};midi.header={sizeof midi,17,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_MIDI,0};midi.data[0]=0x90;midi.data[1]=69;midi.data[2]=100;
  clap_input_events_t input{&midi,[](const clap_input_events_t*)->uint32_t{return 1;},[](const clap_input_events_t* e,uint32_t)->const clap_event_header_t*{return &static_cast<clap_event_midi_t*>(e->ctx)->header;}};
  clap_process_t p{};p.audio_outputs=&out;p.audio_outputs_count=1;p.frames_count=257;p.in_events=note?&input:nullptr;
  require(synth.process(&p)==CLAP_PROCESS_CONTINUE,synth.lastError.c_str());float peak=0;for(auto v:l){require(std::isfinite(v),"Finite host output");peak=std::max(peak,std::abs(v));}return peak;
}
int main(int argc,char** argv){try{
  require(argc==2,"Resource path required");
  clap_host_t host={CLAP_VERSION,nullptr,"Offline regression","Rustias","","0.1.0",[](const clap_host_t*,const char*)->const void*{return nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
  Instrument a(&host,argv[1]);
  a.queue(R"({"type":"control","timbre":0,"parameter":9,"value":127})");
  Json track={{"enabled",true},{"length",128},{"resolution","1/16"},{"steps",Json::array()}};
  for(int i=0;i<128;i++)track["steps"].push_back({{"notes",i?Json::array():Json::array({60,64,67})},{"velocity",100},{"gate",75}});
  a.queue(Json{{"type","sequencer"},{"config",{{"version",1},{"tracks",Json::array({track,track,track,track})}}}}.dump());
  a.queue(R"({"type":"library-sample","asset":1,"data":[0,0.25,-0.25,0]})");
  a.queue(R"({"type":"library-reset"})");
  a.queue(R"({"type":"library-sample","asset":2,"data":[0,0.15,-0.15,0]})");
  a.uiMessage(R"({"kind":"session","data":{"snapshot":{"volume":42,"macros":null},"programDirty":true}})");
  require(a.activate(48000),a.lastError.c_str());
  float peak=render(a,true);for(int i=0;i<80;i++)peak=std::max(peak,render(a));require(peak>.001,"MIDI reaches shared engine through host");
  auto saved=a.save();auto state=Json::parse(saved);require(state["parameters"][8]==42,"Output level is saved");require(state["session"]["programDirty"]==true,"Complete UI session is saved");
  require(std::count_if(state["messages"].begin(),state["messages"].end(),[](auto& e){return e["type"]=="library-sample";})==1,"Library reset removes stale PCM from state");
  Instrument b(&host,argv[1]);require(b.restore(saved),"State loads before activation");require(b.save()==saved,"Complete state round trip includes sequencer and sample data");require(b.activate(44100),b.lastError.c_str());require(render(b,true)>=0,"Restored state renders at a different sample rate");
  const auto before=b.save();require(!b.restore("{}"),"Invalid state is rejected");require(b.save()==before,"Invalid state leaves project intact");
  b.parameter(8,0,false);require(b.restore(saved),"State can load while active");render(b);require(b.parameters[8]==42,"Restored automation is applied");
  require(b.activate(96000),b.lastError.c_str());render(b);require(b.save()==saved,"Sample rate changes preserve project state");
  std::cout<<"Offline host: timestamped MIDI, state/PCM/sequencer round trip, invalid-state isolation and sample-rate changes passed.\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
