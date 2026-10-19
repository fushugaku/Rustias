// Native DAW adapter. All sound and modulation still come from the canonical
// Rust crates. A script VM is never entered by an audio or MIDI callback.
#include "Runtime.h"
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstring>
#include <deque>
#include <map>
#include <mutex>
#include <optional>
#include <set>
#include <stdexcept>
#include <thread>
using Json=nlohmann::json;

struct Runtime::Direct {
  Runtime& owner;
  NativeEngine engine;
  std::map<std::string,unsigned,std::less<>> methods;
  double rate,gain=.3,phase=0,currentLeft=0,currentRight=0,nextLeft=0,nextRight=0;
  std::array<float,256> block{};
  unsigned index=0,length=0,nativePhase=0;
  uint64_t callbacks=0,audibleFrames=0,snapshotFrames=0,auditionFrame=0;
  double statsFrames=0;
  double peak=0;
  bool statsEnabled=true;
  std::optional<std::array<unsigned char,3>> pendingMidi;
  std::map<unsigned,std::array<unsigned char,3>> pendingVolumes;
  struct Profile {unsigned id,timbre;std::string source;};
  std::map<std::pair<unsigned,std::string>,Profile> profiles;
  using Note=std::pair<unsigned,Json>;
  std::set<Note> manualNotes,auditionNotes;
  std::vector<Note> audition;
  double releaseAt=0;
  std::string recording;
  uint64_t recordedFrames=0,recordIndex=0;
  std::vector<int16_t> recordingPCM;
  static constexpr size_t chunkSamples=32768;

  // Single audio producer, single formatter consumer. Formatting JSON / large
  // PCM arrays cannot hold up a DAW callback. Slow UI consumers cannot lock it.
  struct Packet {Json message;std::vector<int16_t> pcm;};
  std::array<Packet,256> packets;
  std::atomic<unsigned> read{0},write{0};
  std::atomic<bool> quitting{false};
  std::mutex consumerMutex,readyMutex,wakeMutex;
  std::condition_variable wake;
  std::vector<std::string> ready;
  std::thread formatter;

  Direct(Runtime& r,double sampleRate):owner(r),rate(sampleRate){
    for(unsigned i=0;i<rustias_native_method_count();i++)methods.emplace(rustias_native_method_name(i),i);
    call("rustias_init");
    post({{"type","ready"},{"sampleRate",rate}});
    formatter=std::thread([this]{
      while(!quitting.load(std::memory_order_acquire)){
        pump();
        std::unique_lock lock(wakeMutex);
        wake.wait_for(lock,std::chrono::milliseconds(2),[this]{return quitting.load()||read.load()!=write.load();});
      }
      pump();
    });
  }
  ~Direct(){quitting.store(true,std::memory_order_release);wake.notify_one();if(formatter.joinable())formatter.join();}
  double call(const char* name,std::initializer_list<double> args={}){
    double result=0;auto found=methods.find(name);
    if(found==methods.end()||!rustias_native_call(engine.handle,found->second,args.begin(),unsigned(args.size()),&result))
      throw std::runtime_error(std::string("Native engine rejected ")+name);
    return result;
  }
  unsigned stage(const Json& value){
    const auto text=value.dump();
    if(text.size()>size_t(call("rustias_preset_capacity"))&&!call("rustias_preset_reserve",{double(text.size())}))throw std::runtime_error("Native program buffer could not grow");
    const auto token=uint32_t(call("rustias_preset_buffer"));
    if(!rustias_native_write(engine.handle,token,reinterpret_cast<const uint8_t*>(text.data()),text.size()))throw std::runtime_error("Invalid native program buffer");
    return unsigned(text.size());
  }
  Json json(const char* method){
    const auto size=size_t(call(method));const auto token=uint32_t(call("rustias_preset_buffer"));std::string bytes(size,'\0');
    if(!rustias_native_read(engine.handle,token,reinterpret_cast<uint8_t*>(bytes.data()),size))throw std::runtime_error("Invalid native JSON buffer");
    return Json::parse(bytes);
  }
  bool post(Json message,std::vector<int16_t> pcm={}){
    const auto w=write.load(std::memory_order_relaxed);
    const auto next=(w+1)%packets.size();
    if(next==read.load(std::memory_order_acquire))return false;
    packets[w]={std::move(message),std::move(pcm)};write.store(unsigned(next),std::memory_order_release);wake.notify_one();return true;
  }
  void pump(){
    std::lock_guard consumer(consumerMutex);
    auto r=read.load(std::memory_order_relaxed);
    while(r!=write.load(std::memory_order_acquire)){
      auto packet=std::move(packets[r]);packets[r]={};
      r=(r+1)%packets.size();read.store(r,std::memory_order_release);
      if(!packet.pcm.empty())packet.message["pcm"]=packet.pcm;
      auto text=packet.message.dump();std::lock_guard lock(readyMutex);ready.push_back(std::move(text));
    }
  }
  std::vector<std::string> take(bool realtime){
    if(!realtime)pump();
    std::unique_lock lock(readyMutex,std::defer_lock);
    if(realtime){if(!lock.try_lock())return {};}else lock.lock();
    std::vector<std::string> result;result.swap(ready);return result;
  }
  void warning(const char* text){post({{"type","warning"},{"message",text}});}
  Json modulationStatus(){
    auto status=json("rustias_modulation_status");
    for(unsigned t=0;t<8;t++)for(unsigned lane=0;lane<6;lane++){
      const auto position=int(call("rustias_poly_motion_position",{double(t),double(lane)}));
      if(position>=0){status["positions"][t][lane]=position;status["running"]=true;}
    }
    return status;
  }
  Json stats(){return {{"frames",call("rustias_frames")},{"voices",call("rustias_voices")},{"capacity",call("rustias_voice_capacity")},{"failed",owner.failure},{"backend","native-rust"},{"modulation",modulationStatus()}};}
  void snapshot(bool midi=false){
    auto program=json("rustias_save");Json volumes=Json::array();
    for(auto& [_,p]:profiles)volumes.push_back({{"timbre",p.timbre},{"source",p.source},{"value",call("rustias_library_value",{double(p.id),117})}});
    Json message={{"type","state"},{"program",std::move(program)},{"libraryVolumes",std::move(volumes)}};
    if(midi&&pendingMidi){
      message["midi"]=*pendingMidi;message["midiBatch"]=Json::array();
      for(auto& [_,bytes]:pendingVolumes)message["midiBatch"].push_back(bytes);
      message["program"].erase("effects");pendingMidi.reset();pendingVolumes.clear();snapshotFrames=0;
    }
    post(std::move(message));
  }
  void flushMidi(){if(pendingMidi)snapshot(true);}
  void midi(unsigned char a,unsigned char b,unsigned char c){
    // NativeHost restores / rebases modulation, handles KeySync and all channel
    // receive rules. Do not run the browser note sequencer in a DAW.
    call("rustias_modulation_restore");call("rustias_midi",{double(a),double(b),double(c)});
    if((a&240)==176||(a&240)==224){pendingMidi=std::array{a,b,c};if((a&240)==176&&b==7)pendingVolumes[a&15]=*pendingMidi;}
  }
  void note(const Note& voice,unsigned velocity,bool preview=false){
    if(preview&&velocity&&voice.second.is_number_integer()){
      const auto pitch=voice.second.get<int>();const auto t=double(voice.first);
      if(pitch<call("rustias_value",{t,119})||pitch>call("rustias_value",{t,120}))return;
    }
    auto& owners=preview?auditionNotes:manualNotes;
    const bool before=manualNotes.contains(voice)||auditionNotes.contains(voice);
    if(velocity)owners.insert(voice);else owners.erase(voice);
    const bool after=manualNotes.contains(voice)||auditionNotes.contains(voice);
    if((velocity&&!before)||(!after&&before)){
      auto [t,n]=voice;
      if(n.is_string()){auto found=profiles.find({t,n.get<std::string>()});if(found!=profiles.end())call("rustias_library_note",{double(t),double(found->second.id),double(velocity)});}
      else if(n.is_number_integer())call("rustias_note",{double(t),n.get<double>(),double(velocity)});
    }
  }
  void stopAudition(){for(auto& voice:audition)note(voice,0,true);audition.clear();}
  void auditionBlock(unsigned frames){if(!audition.empty()&&auditionFrame>=releaseAt)stopAudition();auditionFrame+=frames;}
  void resetOutput(){index=length=0;nativePhase=0;phase=0;currentLeft=currentRight=nextLeft=nextRight=0;}
  void flushRecording(){
    if(recordingPCM.empty())return;
    if(!post({{"type","record-chunk"},{"id",recording},{"index",recordIndex++},{"frames",recordedFrames}},std::move(recordingPCM)))throw std::runtime_error("Recording output queue is full");
    recordingPCM={};recordingPCM.reserve(chunkSamples);
  }
  void stopRecording(const Json& message){
    if(recording.empty()||message.value("id",recording)!=recording)return;
    flushRecording();post({{"type","record-stopped"},{"id",recording},{"frames",recordedFrames}});recording.clear();recordingPCM.clear();
  }
  void sample(const Json& data,bool library){
    const auto id=data.at(library?"asset":"instrument").get<unsigned>();const auto pcm=data.at("data").get<std::vector<float>>();
    const auto token=uint32_t(call(library?"rustias_library_sample_buffer":"rustias_sample_buffer",{double(id),double(pcm.size())}));
    bool ok=token&&rustias_native_write(engine.handle,token,reinterpret_cast<const uint8_t*>(pcm.data()),pcm.size()*sizeof(float));
    if(ok)ok=library?call("rustias_library_sample_commit",{double(id),double(pcm.size())})!=0:call("rustias_sample_commit",{double(id),double(pcm.size()),data.value("mode",0.)})!=0;
    post({{"type","sample-ready"},{"request",data.value("request",Json())},{"ok",ok}});
  }
  void message(const Json& data){
    const auto type=data.value("type",std::string());
    if(type=="control"||type=="osc3-control"||type=="drum-control"||type=="library-control"||type=="effect"||type=="effect-chain"||type=="chain-effect"||type=="circuit"||type=="gain"||type=="drum-gain"||type=="load")call("rustias_modulation_restore");
    if(type=="control"){
      const auto id=data.at("parameter").get<unsigned>();const double t=data.at("timbre"),v=data.at("value");
      if(!(id>=171&&id<179?call("rustias_osc3_control",{t,double(id-171),v}):call("rustias_control",{t,double(id),v})))snapshot();
      call("rustias_modulation_accept_values");
    }else if(type=="osc3-control"){
      if(!call("rustias_osc3_control",{data.at("timbre"),data.at("control"),data.at("value")})){snapshot();warning("OSC 3 could not apply this setting. Reload the instrument.");}
      call("rustias_modulation_accept_values");
    }else if(type=="drum-control"){
      call("rustias_drum_control",{data.at("instrument"),data.at("parameter"),data.at("value")});call("rustias_modulation_accept_values");
    }else if(type=="library-control"){
      call("rustias_library_control",{data.at("id"),data.at("parameter"),data.at("value")});call("rustias_library_sync");call("rustias_modulation_accept_values");
    }else if(type=="midi"){auto bytes=data.at("bytes");midi(bytes.at(0),bytes.at(1),bytes.at(2));}
    else if(type=="note")note({data.at("timbre"),data.at("note")},data.at("velocity"));
    else if(type=="drum")call("rustias_drum_pad",{data.at("instrument"),data.at("velocity")});
    else if(type=="transport")call("rustias_modulation_transport",{data.value("playing",false)?1.:0.,data.at("tempo"),data.value("beat",0.),data.value("seek",false)?1.:0.});
    else if(type=="gain"){gain=std::clamp(data.at("value").get<double>(),0.,1.);call("rustias_modulation_parameter",{8,gain*100});}
    else if(type=="drum-gain")call("rustias_drum_gain",{data.at("value")});
    else if(type=="modulation"){
      const auto& state=data.at("state");auto n=stage(state.at("config"));if(!call("rustias_poly_motion",{double(n)}))throw std::runtime_error("The engine rejected polyphonic motion settings.");
      n=stage(state);if(!call("rustias_modulation_state",{double(n)}))throw std::runtime_error("The native modulation engine rejected this program.");
    }else if(type=="effect"){
      const auto n=stage(data.at("program"));if(!call("rustias_effect",{data.at("slot"),double(n)}))warning("The engine rejected these effect settings.");
    }else if(type=="effect-chain"||type=="chain-effect"){
      const auto n=stage(data.at(type=="effect-chain"?"chain":"program"));if(!call(type=="effect-chain"?"rustias_effect_chain":"rustias_chain_effect",{data.at("timbre"),double(n)}))warning("The engine rejected this timbre effect chain.");
    }else if(type=="circuit"){
      const auto n=stage(data.at("circuit"));if(!call("rustias_circuit",{data.at("timbre"),double(n)}))warning("The engine rejected this modular patch.");
    }else if(type=="load"){
      pendingMidi.reset();pendingVolumes.clear();snapshotFrames=0;call("rustias_modulation_stop");
      const auto n=stage(data.at("program"));
      if(!call("rustias_load",{double(n)})){warning("The Rust engine rejected this program.");snapshot();}
      else {const auto unavailable=data.value("unavailable",Json::object());call("rustias_rdl_mute",{unavailable.value("timbres",0.),unavailable.value("drums",0.)});stopAudition();manualNotes.clear();auditionNotes.clear();call("rustias_modulation_stop");resetOutput();}
    }else if(type=="rdl-muted")call("rustias_rdl_mute",{data.at("timbres"),data.at("drums")});
    else if(type=="stop"){
      call("rustias_modulation_stop");stopAudition();manualNotes.clear();auditionNotes.clear();call("rustias_stop");flushMidi();
    }else if(type=="sample"||type=="library-sample")sample(data,type=="library-sample");
    else if(type=="sample-clear")call("rustias_sample_clear",{data.at("instrument")});
    else if(type=="sample-mode")call("rustias_sample_mode",{data.at("instrument"),data.at("mode")});
    else if(type=="library-reset"){call("rustias_library_reset");profiles.clear();}
    else if(type=="library-profile"){
      const double id=data.at("id"),t=data.at("timbre");if(!call("rustias_library_profile",{id,data.at("asset"),t,data.at("mode")}))throw std::runtime_error("The sample profile could not be loaded.");
      const auto& values=data.at("values");for(unsigned i=0;i<values.size();i++)if(!call("rustias_library_control",{id,double(i),values[i]}))throw std::runtime_error("Invalid sample sound parameters.");
      call("rustias_library_sync");const auto source=data.at("source").get<std::string>();profiles[{unsigned(t),source}]={unsigned(id),unsigned(t),source};const auto n=stage(source);
      if(!call("rustias_modulation_profile",{t,id,double(n)}))throw std::runtime_error("The native modulation sample mapping was rejected.");
    }else if(type=="record-start"){
      if(recording.empty()&&data.contains("id")&&data["id"].is_string()){recording=data["id"];recordedFrames=recordIndex=0;recordingPCM.clear();recordingPCM.reserve(chunkSamples);post({{"type","record-started"},{"id",recording},{"sampleRate",rate}});}
    }else if(type=="record-stop")stopRecording(data);
    else if(type=="audition"){
      stopAudition();const unsigned t=data.at("timbre");if(t>=8)throw std::runtime_error("Invalid audition timbre.");const auto& step=data.at("step");const unsigned velocity=step.value("velocity",100);
      // Preserve voice allocation / summation order: ascending numeric notes,
      // then unique sample sources in their original order.
      std::set<int> pitched;for(const auto& n:step.value("notes",Json::array()))pitched.insert(n.get<int>());
      std::vector<Json> notes;for(auto n:pitched)notes.emplace_back(n);std::set<std::string> samples;
      for(const auto& sample:step.value("samples",Json::array()))if(samples.insert(sample.get<std::string>()).second)notes.push_back(sample);
      for(const auto& n:notes){audition.emplace_back(t,n);note(audition.back(),velocity,true);}
      const auto resolution=data.value("resolution",std::string("1/16"));const auto slash=resolution.find('/');const auto beats=4*std::stod(resolution.substr(0,slash))/std::stod(resolution.substr(slash+1));
      releaseAt=auditionFrame+std::max(128.,48000.*60/(call("rustias_value",{0,89})/10)*beats*step.value("gate",75.)/100);
    }
    // Browser note sequencer messages are deliberately inert in DAW plugins.
  }
  void nativeBlock(unsigned needed){
    if(index!=length)return;
    const unsigned count=std::min(needed,128-nativePhase);auditionBlock(count);
    const auto token=uint32_t(call("rustias_render_frames",{double(count)}));
    if(!rustias_native_read(engine.handle,token,reinterpret_cast<uint8_t*>(block.data()),sizeof(block)))throw std::runtime_error("Invalid native PCM buffer");
    gain=call("rustias_modulation_gain");nativePhase=(nativePhase+count)%128;length=count;index=0;
  }
  void render(float* left,float* right,unsigned frames){
    if(rate==48000){
      for(unsigned at=0;at<frames;){nativeBlock(frames-at);const auto n=std::min(frames-at,length-index),start=index*2;
        for(unsigned i=0;i<n;i++){left[at+i]=float(double(block[start+i*2])*gain);right[at+i]=float(double(block[start+i*2+1])*gain);}index+=n;at+=n;
      }
    }else for(unsigned i=0;i<frames;i++){
      const auto fraction=phase/rate;
      left[i]=float((currentLeft+(nextLeft-currentLeft)*fraction)*gain);right[i]=float((currentRight+(nextRight-currentRight)*fraction)*gain);
      phase+=48000;
      while(phase>=rate){phase-=rate;currentLeft=nextLeft;currentRight=nextRight;nativeBlock(1+unsigned(std::floor((phase+48000.*(frames-i-1))/rate)));nextLeft=block[index*2];nextRight=block[index*2+1];index++;}
    }
    if(!recording.empty())for(unsigned i=0;i<frames;i++){
      for(float v:{left[i],right[i]}){const double x=std::isfinite(v)?std::clamp(double(v),-1.,1.):0.;recordingPCM.push_back(int16_t(std::floor(x*(x<0?32768:32767)+.5)));}
      recordedFrames++;if(recordingPCM.size()==chunkSamples)flushRecording();
    }
    callbacks++;
    if(statsEnabled){
      for(unsigned i=0;i<frames;i++){peak=std::max({peak,std::abs(double(left[i])),std::abs(double(right[i]))});if(left[i]!=0||right[i]!=0)audibleFrames++;}
      statsFrames+=frames;if(statsFrames>=rate/20){statsFrames=std::fmod(statsFrames,rate/20);auto status=stats();status["type"]="stats";status["audibleFrames"]=audibleFrames;status["peak"]=peak;status["callbacks"]=callbacks;status["sequence"]=nullptr;post(std::move(status));peak=0;}
    }
    snapshotFrames=std::min(uint64_t(std::ceil(rate/30)),snapshotFrames+frames);if(snapshotFrames>=rate/30)flushMidi();
  }
};
Runtime::Runtime(const std::string&,double rate):direct(std::make_unique<Direct>(*this,rate)){}
Runtime::~Runtime()=default;
void Runtime::message(const std::string& text){try{direct->message(Json::parse(text));}catch(const std::exception& e){failure=true;errorText=e.what();throw;}}
void Runtime::midi(unsigned char a,unsigned char b,unsigned char c){direct->midi(a,b,c);}
void Runtime::parameter(unsigned index,double value){direct->call("rustias_modulation_parameter",{double(index),value});}
void Runtime::telemetry(bool enabled){if(direct->statsEnabled!=enabled){direct->statsEnabled=enabled;direct->statsFrames=0;direct->peak=0;}}
void Runtime::render(float* left,float* right,unsigned frames){if(failure)throw std::runtime_error(errorText);direct->render(left,right,frames);}
std::vector<std::string> Runtime::takeMessages(){return direct->take(false);}
std::vector<std::string> Runtime::takeRealtimeMessages(){return direct->take(true);}
std::string Runtime::stats(){return direct->stats().dump();}
