// Loads the real editor/worker/storage bridge. Audio remains in discarded buffers.
#include "Instrument.h"
#include "Editor.h"
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>
#include <nlohmann/json.hpp>
#include <iostream>
static uint64_t modulationSerial(Instrument& instrument){
  std::lock_guard lock(instrument.stateMutex);
  for(const auto& [_,event]:instrument.remembered)if(nlohmann::json::parse(event.second).value("type",std::string())=="modulation")return event.first;
  return 0;
}
static void pump(Instrument& instrument){
  float l[256],r[256];float* channels[]={l,r};clap_audio_buffer_t out{};out.data32=channels;out.channel_count=2;clap_process_t p{};p.frames_count=256;p.audio_outputs=&out;p.audio_outputs_count=1;instrument.process(&p);
  [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
}
static bool editCutoff(WKWebView* web,int value){
  __block bool done=false,accepted=false;
  auto script=[NSString stringWithFormat:@"(()=>{const input=document.querySelector('#parameter-1');if(!input||input.disabled)return false;input.value='%d';input.dispatchEvent(new Event('input',{bubbles:true}));return true;})()",value];
  [web evaluateJavaScript:script completionHandler:^(id result,NSError* error){accepted=!error&&[result boolValue];done=true;}];
  auto deadline=[NSDate dateWithTimeIntervalSinceNow:3];while(!done&&deadline.timeIntervalSinceNow>0)[NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
  return done&&accepted;
}
int main(int argc,char** argv){@autoreleasepool{
  if(argc!=2)return 1;setenv("RUSTIAS_UI_CHECK","1",1);[NSApplication sharedApplication];
  clap_host_t host={CLAP_VERSION,nullptr,"Silent editor check","Rustias","","0.1.0",[](const clap_host_t*,const char*)->const void*{return nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
  Instrument instrument(&host,argv[1]);if(!instrument.activate(48000)){std::cerr<<instrument.lastError;return 1;}
  auto window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,1100,800) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
  instrument.editor=std::make_unique<Editor>(instrument);instrument.editor->parent((__bridge void*)window.contentView);instrument.editor->show(true);
  auto deadline=[NSDate dateWithTimeIntervalSinceNow:25];
  while(!instrument.editorReady&&instrument.lastError.empty()&&deadline.timeIntervalSinceNow>0){
    pump(instrument);
  }
  if(!instrument.editorReady){std::cerr<<"Editor failed: "<<instrument.lastError<<'\n';return 1;}
  WKWebView* web=nil;for(NSView* child in window.contentView.subviews)if([child isKindOfClass:WKWebView.class])web=(WKWebView*)child;
  if(!web){std::cerr<<"Editor did not attach its WebView\n";return 1;}
  deadline=[NSDate dateWithTimeIntervalSinceNow:3];while(!modulationSerial(instrument)&&deadline.timeIntervalSinceNow>0)pump(instrument);
  const auto configured=modulationSerial(instrument);if(!configured){std::cerr<<"Editor did not configure native modulation\n";return 1;}
  for(int value:{50,60,70,80}){
    if(!editCutoff(web,value)){std::cerr<<"Editor cutoff input failed\n";return 1;}pump(instrument);
    if(modulationSerial(instrument)!=configured){std::cerr<<"A knob input resent the full modulation configuration\n";return 1;}
  }
  if(!instrument.lastError.empty()){std::cerr<<instrument.lastError<<'\n';return 1;}
  for(int cycle=0;cycle<8;cycle++){
    instrument.editor->show(false);
    if(!web.hidden||web.superview!=window.contentView){std::cerr<<"Hide must keep the editor attached for show without reparenting\n";return 1;}
    instrument.editor->show(true);
    if(web.hidden||web.superview!=window.contentView){std::cerr<<"Show must restore the attached editor\n";return 1;}
  }
  std::cout<<"Native editor, workers and IndexedDB initialized; cutoff gestures reuse modulation configuration and hide/show retains the view, without audio hardware.\n";instrument.editor.reset();return 0;
}}
