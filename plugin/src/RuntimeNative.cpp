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
  std::array<std::array<unsigned char,3>,16> pendingVolumes{};
  unsigned pendingVolumeMask=0;
  struct Profile {unsigned id,timbre;std::string source;};
  std::map<std::pair<unsigned,std::string>,Profile> profiles;
  using Note=std::pair<unsigned,Json>;
  std::set<Note> manualNotes,auditionNotes;
  std::vector<Note> audition;
  double releaseAt=0;
  std::string recording;
  uint64_t recordedFrames=0,recordIndex=0;
  static constexpr size_t chunkSamples=32768;
  struct RecordingPool {
    std::string id;
    // Preserve the full FIFO backlog: 255 queued + one formatting + one writing.
    // Keep flags compact, so preparation does not touch every PCM page.
    std::array<std::array<int16_t,chunkSamples>,257> chunks;
    std::array<std::atomic<bool>,257> available;
    explicit RecordingPool(std::string value):id(std::move(value)){for(auto& flag:available)flag.store(true,std::memory_order_relaxed);}
  };
  std::shared_ptr<RecordingPool> recordingPool;
  int recordingSlot=-1;
  size_t recordingSamples=0;
  struct SnapshotPool {
    std::array<void*,19> data{};
    std::array<std::atomic<bool>,19> available;
    explicit SnapshotPool(void* engine){for(size_t i=0;i<data.size();i++){data[i]=rustias_native_snapshot_create(engine);available[i].store(true);}}
    ~SnapshotPool(){for(auto* p:data)rustias_native_snapshot_destroy(p);}
  };
  std::shared_ptr<SnapshotPool> snapshotPool;
  void prepareSnapshots(){snapshotPool=std::make_shared<SnapshotPool>(engine.handle);}

  // Single audio producer, single formatter consumer. Formatting JSON / large
  // PCM arrays cannot hold up a DAW callback. Slow UI consumers cannot lock it.
  struct Packet {
    Json message;
    std::shared_ptr<SnapshotPool> snapshot;
    std::shared_ptr<RecordingPool> recording;
    NativeSnapshotMetadata metadata;
    unsigned slot=0,snapshotKind=0;
    uint64_t recordIndex=0,recordFrames=0;
    size_t samples=0;
  };
  std::array<Packet,256> packets;
  // Triple-buffer mailbox for mandatory state under queue pressure. The audio
  // side owns back, the formatter owns front; exchange transfers only middle.
  // A pending state can be coalesced without touching either thread's buffer.
  std::array<Packet,3> latestState;
  std::atomic<unsigned> stateMiddle{2};
  unsigned stateBack=0,stateFront=1;
  static constexpr unsigned stateDirty=4;
  std::atomic<unsigned> read{0},write{0};
  std::atomic<bool> quitting{false};
  std::mutex consumerMutex,readyMutex;
  std::atomic<uint64_t> wakeRevision{0};
  std::vector<std::string> ready;
  std::thread formatter;

  Direct(Runtime& r,double sampleRate):owner(r),rate(sampleRate){
    for(unsigned i=0;i<rustias_native_method_count();i++)methods.emplace(rustias_native_method_name(i),i);
    call("rustias_init");
    prepareSnapshots();
    post({{"type","ready"},{"sampleRate",rate}});
    formatter=std::thread([this]{
      auto observed=wakeRevision.load(std::memory_order_acquire);
      for(;;){
        pump();
        if(quitting.load(std::memory_order_acquire))break;
        // Read the generation before draining. A producer between pump() and
        // wait() changes it, so wait cannot lose that notification.
        wakeRevision.wait(observed,std::memory_order_acquire);
        observed=wakeRevision.load(std::memory_order_acquire);
      }
      pump();
    });
  }
  ~Direct(){quitting.store(true,std::memory_order_release);wakeRevision.fetch_add(1,std::memory_order_release);wakeRevision.notify_one();if(formatter.joinable())formatter.join();}
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
  bool enqueue(Packet packet){
    const auto w=write.load(std::memory_order_relaxed);const auto next=(w+1)%packets.size();
    if(next==read.load(std::memory_order_acquire))return false;
    packets[w]=std::move(packet);write.store(unsigned(next),std::memory_order_release);
    wakeRevision.fetch_add(1,std::memory_order_release);wakeRevision.notify_one();return true;
  }
  bool post(Json message){Packet p;p.message=std::move(message);return enqueue(std::move(p));}
  std::string format(const Packet& packet){
    if(packet.snapshot){
      size_t size=0;auto* bytes=rustias_native_snapshot_format(packet.snapshot->data[packet.slot],&packet.metadata,&size);
      if(!bytes)throw std::runtime_error("Invalid native snapshot");
      std::string text(reinterpret_cast<const char*>(bytes),size);
      if(packet.slot<16)packet.snapshot->available[packet.slot].store(true,std::memory_order_release);
      return text;
    }
    if(packet.recording){
      auto& chunk=packet.recording->chunks[packet.slot];
      Json pcm=Json::array();auto& values=pcm.get_ref<Json::array_t&>();values.reserve(packet.samples);
      for(size_t i=0;i<packet.samples;i++)values.push_back(chunk[i]);
      auto text=Json{{"type","record-chunk"},{"id",packet.recording->id},{"index",packet.recordIndex},{"frames",packet.recordFrames},{"pcm",std::move(pcm)}}.dump();
      packet.recording->available[packet.slot].store(true,std::memory_order_release);
      return text;
    }
    return packet.message.dump();
  }
  void deliver(const Packet& packet){auto text=format(packet);std::lock_guard lock(readyMutex);ready.push_back(std::move(text));}
  void pump(){
    std::lock_guard consumer(consumerMutex);
    auto r=read.load(std::memory_order_relaxed);
    while(r!=write.load(std::memory_order_acquire)){
      auto packet=std::move(packets[r]);packets[r]={};
      r=(r+1)%packets.size();read.store(r,std::memory_order_release);
      deliver(packet);
    }
    if(stateMiddle.load(std::memory_order_acquire)&stateDirty){
      const auto published=stateMiddle.exchange(stateFront,std::memory_order_acq_rel);
      stateFront=published&3;
      deliver(latestState[stateFront]);
    }
  }
  Packet metadata(unsigned kind){
    Packet packet;packet.snapshot=snapshotPool;
    auto& meta=packet.metadata;meta.callbacks=callbacks;meta.audibleFrames=audibleFrames;meta.peak=peak;meta.failed=owner.failure;
    if(kind==2&&pendingMidi){meta.hasMidi=1;std::copy(pendingMidi->begin(),pendingMidi->end(),meta.midi);meta.volumeMask=pendingVolumeMask;for(unsigned c=0;c<16;c++)std::copy(pendingVolumes[c].begin(),pendingVolumes[c].end(),meta.volumes[c]);}
    return packet;
  }
  bool captureState(unsigned kind){
    auto packet=metadata(kind);packet.slot=16+stateBack;
    const auto previous=stateMiddle.load(std::memory_order_acquire);
    // A pending full correction must not be downgraded to a MIDI-only state.
    const unsigned capturedKind=(previous&stateDirty)&&latestState[previous&3].snapshotKind==1?1:kind;
    packet.snapshotKind=capturedKind;
    if(!rustias_native_snapshot_capture(engine.handle,snapshotPool->data[packet.slot],capturedKind))return false;
    if(previous&stateDirty){
      // Only read immutable metadata. The consumer never moves/clears mailbox
      // packets; this producer cannot reuse that middle until after exchange.
      const auto& old=latestState[previous&3].metadata;auto& current=packet.metadata;
      for(unsigned c=0;c<16;c++)if((old.volumeMask&(1u<<c))&&!(current.volumeMask&(1u<<c)))std::copy(std::begin(old.volumes[c]),std::end(old.volumes[c]),current.volumes[c]);
      current.volumeMask|=old.volumeMask;
      if(!current.hasMidi&&old.hasMidi){current.hasMidi=1;std::copy(std::begin(old.midi),std::end(old.midi),current.midi);}
    }
    latestState[stateBack]=std::move(packet);
    stateBack=stateMiddle.exchange(stateBack|stateDirty,std::memory_order_acq_rel)&3;
    wakeRevision.fetch_add(1,std::memory_order_release);wakeRevision.notify_one();
    return true;
  }
  bool capture(unsigned kind){
    if(kind!=0&&(stateMiddle.load(std::memory_order_acquire)&stateDirty))return captureState(kind);
    for(unsigned i=0;i<16;i++)if(snapshotPool->available[i].load(std::memory_order_acquire)){
      if(!rustias_native_snapshot_capture(engine.handle,snapshotPool->data[i],kind))return false;
      auto packet=metadata(kind);packet.slot=i;
      snapshotPool->available[i].store(false,std::memory_order_release);
      if(enqueue(std::move(packet)))return true;
      snapshotPool->available[i].store(true,std::memory_order_release);break;
    }
    return kind!=0&&captureState(kind);
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
    if(capture(midi?2:1)&&midi&&pendingMidi){pendingMidi.reset();pendingVolumeMask=0;snapshotFrames=0;}
  }
  void flushMidi(){if(pendingMidi)snapshot(true);}
  void midi(unsigned char a,unsigned char b,unsigned char c){
    // NativeHost restores / rebases modulation, handles KeySync and all channel
    // receive rules. Do not run the browser note sequencer in a DAW.
    call("rustias_modulation_restore");call("rustias_midi",{double(a),double(b),double(c)});
    if((a&240)==176||(a&240)==224){pendingMidi=std::array{a,b,c};if((a&240)==176&&b==7){pendingVolumes[a&15]=*pendingMidi;pendingVolumeMask|=1u<<(a&15);}}
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
    if(!recordingSamples)return;
    Packet packet;packet.recording=recordingPool;packet.slot=unsigned(recordingSlot);packet.samples=recordingSamples;packet.recordIndex=recordIndex++;packet.recordFrames=recordedFrames;
    if(!enqueue(std::move(packet)))throw std::runtime_error("Recording output queue is full");
    recordingSlot=-1;recordingSamples=0;
  }
  void recordFrame(float left,float right){
    if(recordingSlot<0){
      for(unsigned i=0;i<recordingPool->chunks.size();i++)if(recordingPool->available[i].load(std::memory_order_acquire)){recordingSlot=int(i);recordingPool->available[i].store(false,std::memory_order_release);break;}
      if(recordingSlot<0)throw std::runtime_error("Recording output queue is full");
    }
    auto& pcm=recordingPool->chunks[recordingSlot];
    for(float v:{left,right}){const double x=std::isfinite(v)?std::clamp(double(v),-1.,1.):0.;pcm[recordingSamples++]=int16_t(std::floor(x*(x<0?32768:32767)+.5));}
    recordedFrames++;if(recordingSamples==chunkSamples)flushRecording();
  }
  void stopRecording(const Json& message){
    if(recording.empty()||message.value("id",recording)!=recording)return;
    flushRecording();post({{"type","record-stopped"},{"id",recording},{"frames",recordedFrames}});recording.clear();recordingPool.reset();
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
      pendingMidi.reset();pendingVolumeMask=0;snapshotFrames=0;call("rustias_modulation_stop");
      const auto n=stage(data.at("program"));
      if(!call("rustias_load",{double(n)})){prepareSnapshots();warning("The Rust engine rejected this program.");snapshot();}
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
      if(recording.empty()&&data.contains("id")&&data["id"].is_string()){recording=data["id"];recordedFrames=recordIndex=0;recordingPool=std::make_shared<RecordingPool>(recording);recordingSlot=-1;recordingSamples=0;post({{"type","record-started"},{"id",recording},{"sampleRate",rate}});}
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
    if(type=="modulation"||type=="load"||type=="effect-chain"||type=="library-profile"||type=="library-reset")prepareSnapshots();
    // Browser note sequencer messages are deliberately inert in DAW plugins.
  }
  void nativeBlock(unsigned needed){
    if(index!=length)return;
    const unsigned count=std::min(needed,128-nativePhase);auditionBlock(count);
    const auto token=uint32_t(call("rustias_render_frames",{double(count)}));
    if(!rustias_native_read(engine.handle,token,reinterpret_cast<uint8_t*>(block.data()),count*2*sizeof(float)))throw std::runtime_error("Invalid native PCM buffer");
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
    if(!recording.empty())for(unsigned i=0;i<frames;i++)recordFrame(left[i],right[i]);
    callbacks++;
    if(statsEnabled){
      for(unsigned i=0;i<frames;i++){peak=std::max({peak,std::abs(double(left[i])),std::abs(double(right[i]))});if(left[i]!=0||right[i]!=0)audibleFrames++;}
      statsFrames+=frames;if(statsFrames>=rate/20){statsFrames=std::fmod(statsFrames,rate/20);capture(0);peak=0;}
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
