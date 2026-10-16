// Loads the real editor/worker/storage bridge. Audio remains in discarded buffers.
#include "Instrument.h"
#include "Editor.h"
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>
#include <iostream>
int main(int argc,char** argv){@autoreleasepool{
  if(argc!=2)return 1;setenv("RUSTIAS_UI_CHECK","1",1);[NSApplication sharedApplication];
  clap_host_t host={CLAP_VERSION,nullptr,"Silent editor check","Rustias","","0.1.0",[](const clap_host_t*,const char*)->const void*{return nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
  Instrument instrument(&host,argv[1]);if(!instrument.activate(48000)){std::cerr<<instrument.lastError;return 1;}
  auto window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,1100,800) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
  instrument.editor=std::make_unique<Editor>(instrument);instrument.editor->parent((__bridge void*)window.contentView);instrument.editor->show(true);
  auto deadline=[NSDate dateWithTimeIntervalSinceNow:25];
  while(!instrument.editorReady&&instrument.lastError.empty()&&deadline.timeIntervalSinceNow>0){
    float l[256],r[256];float* channels[]={l,r};clap_audio_buffer_t out{};out.data32=channels;out.channel_count=2;clap_process_t p{};p.frames_count=256;p.audio_outputs=&out;p.audio_outputs_count=1;instrument.process(&p);
    [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
  }
  if(!instrument.editorReady){std::cerr<<"Editor failed: "<<instrument.lastError<<'\n';return 1;}
  WKWebView* web=nil;for(NSView* child in window.contentView.subviews)if([child isKindOfClass:WKWebView.class])web=(WKWebView*)child;
  if(!web){std::cerr<<"Editor did not attach its WebView\n";return 1;}
  for(int cycle=0;cycle<8;cycle++){
    instrument.editor->show(false);
    if(!web.hidden||web.superview!=window.contentView){std::cerr<<"Hide must keep the editor attached for show without reparenting\n";return 1;}
    instrument.editor->show(true);
    if(web.hidden||web.superview!=window.contentView){std::cerr<<"Show must restore the attached editor\n";return 1;}
  }
  std::cout<<"Native editor, shared interface, WASM module, RDL worker, recording worker and IndexedDB initialized without audio hardware.\n";instrument.editor.reset();return 0;
}}
