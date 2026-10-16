#include "Instrument.h"
#include "Editor.h"
#include <nlohmann/json.hpp>
#include <filesystem>
#ifdef _WIN32
#include <windows.h>
#endif
#include <algorithm>
#include <cmath>
#include <cstring>
#include <cstdio>
#include <stdexcept>

using Json=nlohmann::json;
static Json parse(const std::string& value){return Json::parse(value,nullptr,false);}
static std::string utf8(const std::filesystem::path& p){auto s=p.u8string();return {reinterpret_cast<const char*>(s.data()),s.size()};}
static Instrument& self(const clap_plugin_t* p){return *static_cast<Instrument*>(p->plugin_data);}
#ifdef _WIN32
#define RUSTIAS_WINDOW_API CLAP_WINDOW_API_WIN32
#define RUSTIAS_WINDOW_HANDLE(window) window->win32
#else
#define RUSTIAS_WINDOW_API CLAP_WINDOW_API_COCOA
#define RUSTIAS_WINDOW_HANDLE(window) window->cocoa
#endif
static std::string bundleResources;
static const char* features[]={CLAP_PLUGIN_FEATURE_INSTRUMENT,CLAP_PLUGIN_FEATURE_SYNTHESIZER,CLAP_PLUGIN_FEATURE_STEREO,nullptr};
static const clap_plugin_descriptor_t descriptor={CLAP_VERSION,"io.github.fushugaku.rustias","Rustias","Rustias","https://github.com/fushugaku/Rustias","","","0.1.1","The shared firmware-free Rustias engine",features};

Instrument::Instrument(const clap_host_t* h,std::string r):host(h),resources(std::move(r)){parameters[8]=30;}
Instrument::~Instrument()=default;
void Instrument::publish(std::string message){auto w=outgoingWrite.load(std::memory_order_relaxed);unsigned next=(w+1)%outgoing.size();if(next==outgoingRead.load(std::memory_order_acquire))return;outgoing[w]=std::move(message);outgoingWrite.store(next,std::memory_order_release);}
bool Instrument::activate(double sampleRate){
  try{
    if(!std::isfinite(sampleRate)||sampleRate<1||sampleRate>2000000)return false;
    if(!runtime||rate!=sampleRate){
      if(runtime){runtime->message("{\"type\":\"record-stop\"}");for(auto& message:runtime->takeMessages())publish(std::move(message));}
      runtime=std::make_unique<Runtime>(resources,sampleRate);rate=sampleRate;
      std::vector<std::pair<uint64_t,std::string>> events;{std::lock_guard lock(stateMutex);for(auto& [_,value]:remembered)events.push_back(value);}
      std::sort(events.begin(),events.end());for(auto& [_,message]:events)runtime->message(message);
    }
    drain();for(unsigned i=0;i<9;i++)runtime->parameter(i,parameters[i]);active=true;haveTransport=false;hostPlaying=false;publish("{\"type\":\"host-rate\",\"sampleRate\":"+std::to_string(rate)+"}");return true;
  }catch(const std::exception& e){lastError=e.what();return false;}
}
std::vector<std::string> Instrument::pull(){std::vector<std::string> result;auto r=outgoingRead.load(std::memory_order_relaxed);while(r!=outgoingWrite.load(std::memory_order_acquire)){result.push_back(std::move(outgoing[r]));r=(r+1)%outgoing.size();}outgoingRead.store(r,std::memory_order_release);return result;}
void Instrument::queue(const std::string& message,bool remember){
  if(remember){auto d=parse(message);auto type=d.is_object()?d.value("type",std::string()):std::string();
    const std::vector<std::string> transient={"note","midi","drum","audition","sequence-play","sequence-stop","sequence-reset","stop","record-start","record-stop"};
    if(!type.empty()&&std::find(transient.begin(),transient.end(),type)==transient.end()){
      Json key=Json::array({type});for(auto field:{"timbre","instrument","asset","id","parameter","control","slot"})if(d.contains(field))key.push_back(d[field]);
      std::lock_guard lock(stateMutex);remembered[key.dump()]={++serial,message};
      if(type=="library-reset")for(auto it=remembered.begin();it!=remembered.end();){auto old=parse(it->second.second);auto t=old.value("type",std::string());if(t=="library-sample"||t=="library-profile"||t=="library-control")it=remembered.erase(it);else ++it;}
    }
  }
  std::lock_guard lock(incomingMutex);incoming.push_back(message);
}
void Instrument::drain(){
  std::deque<std::string> work;{std::unique_lock lock(incomingMutex,std::try_to_lock);if(lock.owns_lock())work.swap(incoming);}
  bool configuration=false;for(auto& s:work){runtime->message(s);configuration|=s.find("\"type\":\"modulation\"")!=std::string::npos||s.find("\"type\":\"load\"")!=std::string::npos;}
  if(restoredParameters.exchange(false)||configuration)for(unsigned i=0;i<9;i++)runtime->parameter(i,parameters[i]);
}
void Instrument::parameter(unsigned id,double value,bool fromUI){
  if(id>=9||!std::isfinite(value))return;value=std::clamp(value,0.,100.);if(id<8)value=std::round(value);auto previous=parameters[id].exchange(value);if(previous==value)return;
  if(fromUI){uiParameters.fetch_or(1u<<id);if(auto p=static_cast<const clap_host_params_t*>(host->get_extension(host,CLAP_EXT_PARAMS)))p->request_flush(host);}
  else if(runtime){runtime->parameter(id,value);publish("{\"type\":\"host-parameter\",\"id\":"+std::to_string(id)+",\"value\":"+std::to_string(value)+"}");}
}
void Instrument::uiMessage(const std::string& message){
  auto d=parse(message);if(!d.is_object())return;auto kind=d.value("kind",std::string());
  if(kind=="audio"&&d.contains("data"))queue(d["data"].dump());
  else if(kind=="editor-ready")editorReady=true;
  else if(kind=="session"&&d.contains("data")){
    {std::lock_guard lock(stateMutex);session=d["data"].dump();}
    auto snapshot=d["data"].value("snapshot",Json::object());auto macros=snapshot.value("macros",Json());auto knobs=macros.is_object()?macros.value("knobs",Json::array()):Json::array();
    for(unsigned i=0;i<std::min<size_t>(8,knobs.size());i++)parameter(i,knobs[i].value("value",0.),true);
    if(snapshot.contains("volume"))parameter(8,snapshot["volume"].get<double>(),true);
    if(auto state=static_cast<const clap_host_state_t*>(host->get_extension(host,CLAP_EXT_STATE)))state->mark_dirty(host);
  }else if(kind=="power"){enabled=d.value("enabled",true);if(!enabled)queue("{\"type\":\"stop\"}",false);}
}
std::string Instrument::save(){
  Json events=Json::array(),values=Json::array();std::vector<std::pair<uint64_t,std::string>> sorted;std::string ui;
  {std::lock_guard lock(stateMutex);for(auto& [_,v]:remembered)sorted.push_back(v);ui=session;}
  std::sort(sorted.begin(),sorted.end());for(auto& [_,s]:sorted){auto event=parse(s);if(!event.is_discarded())events.push_back(std::move(event));}for(auto& v:parameters)values.push_back(v.load());
  auto savedSession=parse(ui);return Json{{"format","rustias-plugin"},{"version",1},{"session",savedSession.is_discarded()?Json():savedSession},{"messages",events},{"parameters",values}}.dump();
}
bool Instrument::restore(const std::string& saved){
  auto d=parse(saved);if(!d.is_object()||d.value("format",std::string())!="rustias-plugin"||d.value("version",0)!=1||!d.contains("messages")||!d["messages"].is_array())return false;
  auto& events=d["messages"];auto values=d.value("parameters",Json::array());if(values.size()!=9||events.size()>100000)return false;
  for(auto& event:events)if(!event.is_object())return false;
  for(auto& v:values)if(!v.is_number()||!std::isfinite(v.get<double>()))return false;
  {std::lock_guard lock(incomingMutex);incoming.clear();incoming.push_back("{\"type\":\"stop\"}");}
  {std::lock_guard lock(stateMutex);remembered.clear();session=d.value("session",Json()).dump();serial=0;}
  for(unsigned i=0;i<9;i++)parameters[i]=std::clamp(values[i].get<double>(),0.,100.);
  for(auto& event:events)queue(event.dump());
  restoredParameters=true;hostPlaying=false;haveTransport=false;
  if(auto params=static_cast<const clap_host_params_t*>(host->get_extension(host,CLAP_EXT_PARAMS)))params->rescan(host,CLAP_PARAM_RESCAN_VALUES);
  if(editor)editor->restore();return true;
}
std::string Instrument::initialState(){std::lock_guard lock(stateMutex);Json events=Json::array(),values=Json::array();for(auto& v:parameters)values.push_back(v.load());for(auto& [_,value]:remembered){auto event=parse(value.second);if(!event.is_discarded())events.push_back(std::move(event));}return Json{{"session",parse(session)},{"sampleRate",rate},{"parameters",values},{"audioState",session!="null"&&!events.empty()?events:Json()}}.dump();}
void Instrument::processEvents(const clap_input_events_t* input,const clap_output_events_t* output){
  if(input)for(unsigned i=0;i<input->size(input);i++){auto h=input->get(input,i);if(h->space_id==CLAP_CORE_EVENT_SPACE_ID&&h->type==CLAP_EVENT_PARAM_VALUE){auto e=reinterpret_cast<const clap_event_param_value_t*>(h);parameter(e->param_id,e->value,false);}}
  auto mask=uiParameters.exchange(0);for(unsigned i=0;i<9;i++)if(mask&(1u<<i)){
    if(runtime)runtime->parameter(i,parameters[i]);
    if(output){clap_event_param_value_t e{};e.header={sizeof e,0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_PARAM_VALUE,CLAP_EVENT_IS_LIVE};e.param_id=i;e.note_id=-1;e.port_index=-1;e.channel=-1;e.key=-1;e.value=parameters[i];output->try_push(output,&e.header);}
  }
}
clap_process_status Instrument::process(const clap_process_t* p){
  if(!runtime||p->audio_outputs_count!=1||p->audio_outputs[0].channel_count<2||!p->audio_outputs[0].data32)return CLAP_PROCESS_ERROR;
  auto left=p->audio_outputs[0].data32[0],right=p->audio_outputs[0].data32[1];
  try{
    drain();
    if(p->transport){auto t=p->transport;bool play=t->flags&CLAP_TRANSPORT_IS_PLAYING;
      if((t->flags&CLAP_TRANSPORT_HAS_TEMPO)&&std::isfinite(t->tempo)&&t->tempo>0&&(!haveTransport||t->tempo!=previousTempo)){runtime->message("{\"type\":\"control\",\"timbre\":0,\"parameter\":89,\"value\":"+std::to_string(std::clamp(int(std::round(t->tempo*10)),100,3000))+"}");previousTempo=t->tempo;}
      if(play&&!hostPlaying)runtime->message("{\"type\":\"sequence-play\"}");
      if(!play&&hostPlaying)runtime->message("{\"type\":\"stop\"}");
      if(play&&haveTransport&&(t->flags&CLAP_TRANSPORT_HAS_BEATS_TIMELINE)){double beat=double(t->song_pos_beats)/CLAP_BEATTIME_FACTOR;if(std::abs(beat-previousBeat-previousFrames/rate*previousTempo/60)>.1)runtime->message("{\"type\":\"sequence-reset\"}");}
      previousBeat=double(t->song_pos_beats)/CLAP_BEATTIME_FACTOR;previousFrames=p->frames_count;haveTransport=true;hostPlaying=play;
    }
    processEvents(nullptr,p->out_events);
    unsigned cursor=0;
    auto render=[&](unsigned end){while(cursor<end){auto n=std::min(128u,end-cursor);if(enabled)runtime->render(left+cursor,right+cursor,n);else{std::fill_n(left+cursor,n,0);std::fill_n(right+cursor,n,0);}cursor+=n;}};
    if(p->in_events)for(unsigned i=0;i<p->in_events->size(p->in_events);i++){
      auto h=p->in_events->get(p->in_events,i);if(h->space_id!=CLAP_CORE_EVENT_SPACE_ID)continue;render(std::clamp(h->time,cursor,p->frames_count));
      if(h->type==CLAP_EVENT_MIDI){auto e=reinterpret_cast<const clap_event_midi_t*>(h);runtime->midi(e->data[0],e->data[1],e->data[2]);}
      else if(h->type==CLAP_EVENT_NOTE_ON||h->type==CLAP_EVENT_NOTE_OFF){auto e=reinterpret_cast<const clap_event_note_t*>(h);runtime->midi((h->type==CLAP_EVENT_NOTE_ON?0x90:0x80)|std::max<int16_t>(0,e->channel),e->key,std::clamp(int(std::round(e->velocity*127)),0,127));}
      else if(h->type==CLAP_EVENT_PARAM_VALUE){auto e=reinterpret_cast<const clap_event_param_value_t*>(h);parameter(e->param_id,e->value,false);}
      else if(h->type==CLAP_EVENT_NOTE_CHOKE)runtime->message("{\"type\":\"stop\"}");
    }
    render(p->frames_count);for(auto& m:runtime->takeMessages())publish(std::move(m));return CLAP_PROCESS_CONTINUE;
  }catch(const std::exception& e){lastError=e.what();std::fill_n(left,p->frames_count,0);std::fill_n(right,p->frames_count,0);publish("{\"type\":\"error\",\"message\":\"Plugin audio stopped; reopen the project to reload its engine.\"}");return CLAP_PROCESS_ERROR;}
}

static const clap_plugin_audio_ports_t audioPorts={
  [](const clap_plugin_t*,bool input)->uint32_t{return input?0:1;},
  [](const clap_plugin_t*,uint32_t index,bool input,clap_audio_port_info_t* info){if(input||index)return false;*info={};info->id=0;std::strcpy(info->name,"Stereo output");info->flags=CLAP_AUDIO_PORT_IS_MAIN;info->channel_count=2;info->port_type=CLAP_PORT_STEREO;info->in_place_pair=CLAP_INVALID_ID;return true;}
};
static const clap_plugin_note_ports_t notePorts={
  [](const clap_plugin_t*,bool input)->uint32_t{return input?1:0;},
  [](const clap_plugin_t*,uint32_t i,bool input,clap_note_port_info_t* info){if(!input||i)return false;*info={};info->id=0;info->supported_dialects=CLAP_NOTE_DIALECT_MIDI|CLAP_NOTE_DIALECT_CLAP;info->preferred_dialect=CLAP_NOTE_DIALECT_MIDI;std::strcpy(info->name,"MIDI · channels 1–8");return true;}
};
static const clap_plugin_params_t params={
  [](const clap_plugin_t*)->uint32_t{return 9;},
  [](const clap_plugin_t*,uint32_t index,clap_param_info_t* info){if(index>=9)return false;*info={};info->id=index;info->flags=CLAP_PARAM_IS_AUTOMATABLE|CLAP_PARAM_REQUIRES_PROCESS|(index<8?CLAP_PARAM_IS_STEPPED:0);info->min_value=0;info->max_value=100;info->default_value=index==8?30:0;std::snprintf(info->name,sizeof info->name,index==8?"Output level":"Macro %u",index+1);return true;},
  [](const clap_plugin_t* p,clap_id id,double* v){if(id>=9)return false;*v=self(p).parameters[id];return true;},
  [](const clap_plugin_t*,clap_id id,double v,char* out,uint32_t n){if(id>=9)return false;std::snprintf(out,n,"%.1f %%",v);return true;},
  [](const clap_plugin_t*,clap_id id,const char* s,double* out){if(id>=9)return false;char* e;*out=std::strtod(s,&e);return e!=s&&std::isfinite(*out)&&*out>=0&&*out<=100;},
  [](const clap_plugin_t* p,const clap_input_events_t* in,const clap_output_events_t* out){auto& s=self(p);try{if(s.runtime)s.drain();s.processEvents(in,out);}catch(const std::exception& e){s.lastError=e.what();}}
};
static const clap_plugin_state_t state={
  [](const clap_plugin_t* p,const clap_ostream_t* stream){auto data=self(p).save();size_t done=0;while(done<data.size()){auto n=stream->write(stream,data.data()+done,data.size()-done);if(n<=0)return false;done+=n;}return true;},
  [](const clap_plugin_t* p,const clap_istream_t* stream){std::string data;char chunk[65536];for(;;){auto n=stream->read(stream,chunk,sizeof chunk);if(n<0||data.size()+std::max<int64_t>(0,n)>128u*1024*1024)return false;if(!n)break;data.append(chunk,n);}return self(p).restore(data);}
};
static const clap_plugin_gui_t gui={
  [](const clap_plugin_t*,const char* api,bool floating){return !floating&&api&&std::strcmp(api,RUSTIAS_WINDOW_API)==0;},
  [](const clap_plugin_t*,const char** api,bool* floating){*api=RUSTIAS_WINDOW_API;*floating=false;return true;},
  [](const clap_plugin_t* p,const char* api,bool floating){if(floating||!api||std::strcmp(api,RUSTIAS_WINDOW_API))return false;auto& s=self(p);if(!s.editor)s.editor=std::make_unique<Editor>(s);return true;},
  [](const clap_plugin_t* p){if(self(p).editor)self(p).editor->show(false);},
  [](const clap_plugin_t*,double){return true;},
  [](const clap_plugin_t* p,uint32_t* w,uint32_t* h){*w=self(p).width;*h=self(p).height;return true;},
  [](const clap_plugin_t*){return true;},
  [](const clap_plugin_t*,clap_gui_resize_hints_t* hints){*hints={true,true,false,0,0};return true;},
  [](const clap_plugin_t*,uint32_t* w,uint32_t* h){*w=std::clamp(*w,800u,2400u);*h=std::clamp(*h,600u,1800u);return true;},
  [](const clap_plugin_t* p,uint32_t w,uint32_t h){auto& s=self(p);s.width=w;s.height=h;if(s.editor)s.editor->size(w,h);return true;},
  [](const clap_plugin_t* p,const clap_window_t* window){return self(p).editor&&window->api&&!std::strcmp(window->api,RUSTIAS_WINDOW_API)&&self(p).editor->parent(RUSTIAS_WINDOW_HANDLE(window));},
  [](const clap_plugin_t*,const clap_window_t*){return false;},
  [](const clap_plugin_t*,const char*){},
  [](const clap_plugin_t* p){if(!self(p).editor)return false;self(p).editor->show(true);return true;},
  [](const clap_plugin_t* p){if(self(p).editor)self(p).editor->show(false);return true;}
};
static const clap_plugin_latency_t latency={[](const clap_plugin_t*)->uint32_t{return 128;}};
static const clap_plugin_tail_t tail={[](const clap_plugin_t*)->uint32_t{return INT32_MAX;}};
static const clap_plugin_t* create(const clap_plugin_factory_t*,const clap_host_t* host,const char* id){
  if(!host||!clap_version_is_compatible(host->clap_version)||std::strcmp(id,descriptor.id))return nullptr;
  auto s=new Instrument(host,bundleResources);s->api={&descriptor,s,
    [](const clap_plugin_t*){return true;},
    [](const clap_plugin_t* p){delete &self(p);},
    [](const clap_plugin_t* p,double rate,uint32_t,uint32_t){return self(p).activate(rate);},
    [](const clap_plugin_t* p){self(p).active=false;},
    [](const clap_plugin_t*){return true;},
    [](const clap_plugin_t* p){if(self(p).runtime)try{self(p).runtime->message("{\"type\":\"stop\"}");}catch(...){}},
    [](const clap_plugin_t* p){if(self(p).runtime)try{self(p).runtime->message("{\"type\":\"stop\"}");}catch(...){}},
    [](const clap_plugin_t* p,const clap_process_t* process){return self(p).process(process);},
    [](const clap_plugin_t*,const char* id)->const void*{if(!std::strcmp(id,CLAP_EXT_AUDIO_PORTS))return &audioPorts;if(!std::strcmp(id,CLAP_EXT_NOTE_PORTS))return &notePorts;if(!std::strcmp(id,CLAP_EXT_PARAMS))return &params;if(!std::strcmp(id,CLAP_EXT_STATE))return &state;if(!std::strcmp(id,CLAP_EXT_GUI))return &gui;if(!std::strcmp(id,CLAP_EXT_LATENCY))return &latency;if(!std::strcmp(id,CLAP_EXT_TAIL))return &tail;return nullptr;},
    [](const clap_plugin_t* p){if(self(p).editor)self(p).editor->poll();}
  };return &s->api;
}
static const clap_plugin_factory_t factory={[](const clap_plugin_factory_t*)->uint32_t{return 1;},[](const clap_plugin_factory_t*,uint32_t i)->const clap_plugin_descriptor_t*{return i?nullptr:&descriptor;},create};
extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry={CLAP_VERSION,
  [](const char* path){
    auto p=std::filesystem::u8path(path?path:"");
#ifdef _WIN32
    HMODULE module=nullptr;GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,reinterpret_cast<LPCWSTR>(&clap_entry),&module);
    wchar_t file[32768];if(GetModuleFileNameW(module,file,32768))p=file;
    bundleResources=utf8(p.extension()==".clap"?p.parent_path()/"Rustias.resources":p.parent_path().parent_path()/"Resources");
#else
    while(p.has_parent_path()&&p.extension()!=".clap"&&p!=p.parent_path())p=p.parent_path();bundleResources=utf8(p/"Contents/Resources");
#endif
    return true;
  },
  [](){},[](const char* id)->const void*{return !std::strcmp(id,CLAP_PLUGIN_FACTORY_ID)?&factory:nullptr;}
};
