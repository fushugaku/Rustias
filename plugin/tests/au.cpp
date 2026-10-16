// Direct AU host: registration stays inside this process, output stays in memory.
#include <AudioToolbox/AudioToolbox.h>
#include <CoreFoundation/CoreFoundation.h>
#include <cmath>
#include <iostream>
#include <stdexcept>
#include <algorithm>
static void require(bool b,const char* message){if(!b)throw std::runtime_error(message);}
static void check(OSStatus status,const char* message){if(status)throw std::runtime_error(std::string(message)+": "+std::to_string(status));}
int main(int argc,char** argv){try{
  require(argc==2,"AU bundle path required");auto path=CFStringCreateWithCString(nullptr,argv[1],kCFStringEncodingUTF8);auto url=CFURLCreateWithFileSystemPath(nullptr,path,kCFURLPOSIXPathStyle,true);CFRelease(path);
  auto bundle=CFBundleCreate(nullptr,url);CFRelease(url);require(bundle&&CFBundleLoadExecutable(bundle),"Load the actual AU bundle");
  auto entries=CFBundleGetValueForInfoDictionaryKey(bundle,CFSTR("AudioComponents"));
  require(entries&&CFGetTypeID(entries)==CFArrayGetTypeID()&&CFArrayGetCount(static_cast<CFArrayRef>(entries))==1,"AU registration survives CMake reconfiguration");
  auto registration=static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(static_cast<CFArrayRef>(entries),0));
  for(auto pair:{std::pair{CFSTR("type"),CFSTR("aumu")},std::pair{CFSTR("subtype"),CFSTR("Rsts")},std::pair{CFSTR("manufacturer"),CFSTR("Fshg")},std::pair{CFSTR("factoryFunction"),CFSTR("wrapAsAUV2_inst0Factory")}}){auto value=CFDictionaryGetValue(registration,pair.first);require(value&&CFEqual(value,pair.second),"AU registration identifies its exported factory");}
  auto factory=reinterpret_cast<AudioComponentFactoryFunction>(CFBundleGetFunctionPointerForName(bundle,CFSTR("wrapAsAUV2_inst0Factory")));require(factory,"Exported AU factory");
  AudioComponentDescription desc{'aumu','Rsts','Fshg',0,0};auto component=AudioComponentRegister(&desc,CFSTR("Rustias: direct validation"),256,factory);require(component,"Register the AU inside the offline host");
  for(auto rate:{48000.,44100.}){
    AudioUnit instance=nullptr;check(AudioComponentInstanceNew(component,&instance),"Create AU instance");
    AudioStreamBasicDescription format{};format.mSampleRate=rate;format.mFormatID=kAudioFormatLinearPCM;format.mFormatFlags=kAudioFormatFlagIsFloat|kAudioFormatFlagsNativeEndian|kAudioFormatFlagIsNonInterleaved|kAudioFormatFlagIsPacked;format.mBytesPerPacket=format.mBytesPerFrame=4;format.mFramesPerPacket=1;format.mChannelsPerFrame=2;format.mBitsPerChannel=32;
    check(AudioUnitSetProperty(instance,kAudioUnitProperty_StreamFormat,kAudioUnitScope_Output,0,&format,sizeof format),"Set stereo format");check(AudioUnitInitialize(instance),"Initialize AU");
    check(AudioUnitSetParameter(instance,8,kAudioUnitScope_Global,0,42,0),"Set AU output automation");
    check(MusicDeviceMIDIEvent(instance,0x90,69,100,0),"Send AU MIDI");
    struct Output{UInt32 count=2;AudioBuffer channels[2];} output;
    float left[256],right[256];output.channels[0]={1,sizeof left,left};output.channels[1]={1,sizeof right,right};float peak=0;
    for(int block=0;block<100;block++){AudioTimeStamp time{};time.mFlags=kAudioTimeStampSampleTimeValid;time.mSampleTime=block*256;AudioUnitRenderActionFlags flags=0;check(AudioUnitRender(instance,&flags,&time,0,256,reinterpret_cast<AudioBufferList*>(&output)),"Render AU buffers");for(auto value:left){require(std::isfinite(value),"Finite AU output");peak=std::max(peak,std::abs(value));}}
    require(peak>.0001f,"AU MIDI produces real shared-engine audio");
    CFPropertyListRef saved=nullptr;UInt32 size=sizeof saved;check(AudioUnitGetProperty(instance,kAudioUnitProperty_ClassInfo,kAudioUnitScope_Global,0,&saved,&size),"Save the actual AU state");require(saved,"AU state exists");
    check(AudioUnitUninitialize(instance),"Uninitialize AU");check(AudioComponentInstanceDispose(instance),"Dispose AU");
    check(AudioComponentInstanceNew(component,&instance),"Recreate AU");check(AudioUnitInitialize(instance),"Initialize restored AU");check(AudioUnitSetProperty(instance,kAudioUnitProperty_ClassInfo,kAudioUnitScope_Global,0,&saved,sizeof saved),"Restore AU state");CFRelease(saved);AudioUnitParameterValue restored=0;check(AudioUnitGetParameter(instance,8,kAudioUnitScope_Global,0,&restored),"Read restored AU parameter");require(restored==42,"AU host sees restored automation values");check(AudioUnitUninitialize(instance),"Uninitialize restored AU");check(AudioComponentInstanceDispose(instance),"Dispose restored AU");
    std::cout<<"Actual AU: MIDI, stereo rendering and state restore passed at "<<rate<<" Hz; peak "<<peak<<"\n";
  }
  CFRelease(bundle);
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
