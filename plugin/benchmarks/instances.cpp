// Run each plugin version in a separate process: the AU factory caches its CLAP library.
// Offline AU host. Audio is rendered into discarded memory; no device is opened.
#include <AudioToolbox/AudioToolbox.h>
#include <CoreFoundation/CoreFoundation.h>
#include <nlohmann/json.hpp>
#include <chrono>
#include <iostream>
#include <stdexcept>
#include <algorithm>
#include <vector>
#include <array>
#include <thread>
#include <fstream>
#include <barrier>
#include <atomic>
using Json=nlohmann::json;using Clock=std::chrono::steady_clock;
void check(OSStatus status,const char* message){if(status)throw std::runtime_error(std::string(message)+": "+std::to_string(status));}
struct AU {
 AudioUnit instance=nullptr;double position=0;std::array<float,512> left{},right{};
 AU(AudioComponent component,double rate,int timbres,int voices,int model,bool complex=false){
  check(AudioComponentInstanceNew(component,&instance),"Create AU");
  AudioStreamBasicDescription f{};f.mSampleRate=rate;f.mFormatID=kAudioFormatLinearPCM;f.mFormatFlags=kAudioFormatFlagIsFloat|kAudioFormatFlagsNativeEndian|kAudioFormatFlagIsNonInterleaved|kAudioFormatFlagIsPacked;f.mBytesPerPacket=f.mBytesPerFrame=4;f.mFramesPerPacket=1;f.mChannelsPerFrame=2;f.mBitsPerChannel=32;
  check(AudioUnitSetProperty(instance,kAudioUnitProperty_StreamFormat,kAudioUnitScope_Output,0,&f,sizeof f),"Format");
  check(AudioUnitInitialize(instance),"Initialize AU for state");CFPropertyListRef saved=nullptr;UInt32 size=sizeof saved;check(AudioUnitGetProperty(instance,kAudioUnitProperty_ClassInfo,kAudioUnitScope_Global,0,&saved,&size),"Read initial state");
  auto dictionary=CFDictionaryCreateMutableCopy(nullptr,0,static_cast<CFDictionaryRef>(saved));CFRelease(saved);
  Json controls=Json::array();for(int t=0;t<timbres;t++)for(auto p:{std::pair{71,1},{72,t},{17,127},{18,0},{19,0},{9,127},{3,0},{4,127},{5,127},{7,50},{1,70},{2,40},{189,model},{191,64}})controls.push_back({{"type","control"},{"timbre",t},{"parameter",p.first},{"value",p.second}});
  if(complex)for(auto p:{std::pair{10,3},{11,29},{12,39},{13,1},{14,1},{15,76},{18,127},{19,127},{171,4},{173,22},{174,72},{179,1},{183,76},{185,1},{186,3},{187,127},{189,2},{190,2},{1,116},{2,113},{22,43},{23,116},{25,26},{27,31},{192,22},{116,1}})controls.push_back({{"type","control"},{"timbre",0},{"parameter",p.first},{"value",p.second}});
  std::string state=Json{{"format","rustias-plugin"},{"version",1},{"session",nullptr},{"parameters",Json::array({0,0,0,0,0,0,0,0,30})},{"messages",controls}}.dump();
  auto data=CFDataCreate(nullptr,reinterpret_cast<const UInt8*>(state.data()),state.size());CFDictionarySetValue(dictionary,CFSTR("data"),data);CFRelease(data);
  CFPropertyListRef replacement=dictionary;check(AudioUnitSetProperty(instance,kAudioUnitProperty_ClassInfo,kAudioUnitScope_Global,0,&replacement,sizeof replacement),"Configure AU");CFRelease(dictionary);
  check(AudioUnitInitialize(instance),"Initialize AU");
  for(int t=0;t<timbres;t++)for(int n=0;n<voices;n++)check(MusicDeviceMIDIEvent(instance,0x90|t,48+n,100,0),"MIDI");
 }
 ~AU(){if(instance){AudioUnitUninitialize(instance);AudioComponentInstanceDispose(instance);}}
 void render(unsigned frames,int controllers=-1){if(controllers>=0){check(MusicDeviceMIDIEvent(instance,0xb0,1,controllers%128,0),"Wheel");check(MusicDeviceMIDIEvent(instance,0xe0,0,controllers%128,frames/2),"Pitch");}struct {UInt32 count=2;AudioBuffer channels[2];}out;out.channels[0]={1,frames*4,left.data()};out.channels[1]={1,frames*4,right.data()};AudioTimeStamp time{};time.mFlags=kAudioTimeStampSampleTimeValid;time.mSampleTime=position;position+=frames;AudioUnitRenderActionFlags flags=0;check(AudioUnitRender(instance,&flags,&time,0,frames,reinterpret_cast<AudioBufferList*>(&out)),"Render");}
};
struct Scene {const char* name;int instances,timbres,voices,model;bool complex=false;};
int main(int argc,char** argv){try{
 if(argc<2||argc>5)throw std::runtime_error("Usage: rustias-au-benchmark AU_BUNDLE [BLOCKS=1024] [RESULT.json] [parallel]");
 const bool parallel=argc>4&&std::string(argv[4])=="parallel";
 auto path=CFStringCreateWithCString(nullptr,argv[1],kCFStringEncodingUTF8);auto url=CFURLCreateWithFileSystemPath(nullptr,path,kCFURLPOSIXPathStyle,true);CFRelease(path);auto bundle=CFBundleCreate(nullptr,url);CFRelease(url);if(!bundle||!CFBundleLoadExecutable(bundle))throw std::runtime_error("Load AU");
 auto factory=reinterpret_cast<AudioComponentFactoryFunction>(CFBundleGetFunctionPointerForName(bundle,CFSTR("wrapAsAUV2_inst0Factory")));if(!factory)throw std::runtime_error("AU factory");AudioComponentDescription desc{'aumu','Rsts','Fshg',0,0};auto component=AudioComponentRegister(&desc,CFSTR("Rustias: offline performance"),256,factory);
 Json results=Json::array();int blocks=argc>2?std::stoi(argv[2]):1024;if(blocks<64||blocks>8192)throw std::runtime_error("BLOCKS must be 64..8192");
 const std::vector<Scene> scenes={{"idle",1,1,0,0},{"two-timbres-8",1,2,8,0},{"two-instances-8",2,1,8,0},{"four-instances-8",4,1,8,0},{"mpoly-two-instances-8",2,1,8,9},{"mpoly-four-instances-8",4,1,8,9},{"logic-four-sources-controllers",2,1,6,2,true}};
 for(double rate:{48000.})for(int frames:{32,64,128}){
  std::vector<std::vector<double>> times(scenes.size());
  for(int round=0;round<3;round++)for(size_t i=0;i<scenes.size();i++){
   const auto j=round%2?scenes.size()-1-i:i;auto s=scenes[j];std::vector<std::unique_ptr<AU>> units;for(int n=0;n<s.instances;n++)units.push_back(std::make_unique<AU>(component,rate,s.timbres,s.voices,s.model,s.complex));
   for(int b=0;b<128;b++)for(auto& unit:units)unit->render(frames);
   if(parallel){
    std::barrier start(s.instances+1),finish(s.instances+1);std::vector<std::thread> threads;std::vector<std::exception_ptr> errors(s.instances);
    for(int n=0;n<s.instances;n++)threads.emplace_back([&,n]{for(int b=0;b<blocks;b++){start.arrive_and_wait();try{units[n]->render(frames,s.complex?b:-1);}catch(...){errors[n]=std::current_exception();}finish.arrive_and_wait();}});
    for(int b=0;b<blocks;b++){auto begin=Clock::now();start.arrive_and_wait();finish.arrive_and_wait();times[j].push_back(std::chrono::duration<double,std::micro>(Clock::now()-begin).count());}
    for(auto& thread:threads)thread.join();for(auto& error:errors)if(error)std::rethrow_exception(error);
   }else for(int b=0;b<blocks;b++){auto start=Clock::now();for(auto& unit:units)unit->render(frames,s.complex?b:-1);times[j].push_back(std::chrono::duration<double,std::micro>(Clock::now()-start).count());}
  }
  for(size_t j=0;j<scenes.size();j++){auto samples=times[j];std::sort(samples.begin(),samples.end());double sum=0;for(auto v:samples)sum+=v;results.push_back({{"scene",scenes[j].name},{"parallel",parallel},{"rate",rate},{"frames",frames},{"mean_us",sum/samples.size()},{"median_us",samples[samples.size()/2]},{"p99_us",samples[size_t((samples.size()-1)*.99)]},{"max_us",samples.back()},{"over_budget",std::count_if(samples.begin(),samples.end(),[&](double t){return t>frames/rate*1e6;})}});}
 }
 if(argc>3){std::ofstream file(argv[3]);file<<results.dump(2)<<"\n";if(!file)throw std::runtime_error("Cannot write benchmark results");}else std::cout<<results.dump(2)<<"\n";CFRelease(bundle);
}catch(const std::exception& e){std::cerr<<e.what()<<"\n";return 1;}}
