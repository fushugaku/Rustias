// UI validation host: renders into discarded memory, never opens an audio device.
#include "Instrument.h"
#include "Editor.h"
#import <Cocoa/Cocoa.h>
int main(){@autoreleasepool{
  freopen(RUSTIAS_PREVIEW_RESOURCES "/../ui-errors.log","a",stderr);
  [NSApplication sharedApplication];[NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
  NSMenu* menu=[NSMenu new];NSMenuItem* appMenu=[NSMenuItem new];NSMenu* items=[NSMenu new];[items addItemWithTitle:@"Quit silent preview" action:@selector(terminate:) keyEquivalent:@"q"];appMenu.submenu=items;[menu addItem:appMenu];NSApp.mainMenu=menu;
  clap_host_t host={CLAP_VERSION,nullptr,"Silent validation host","Rustias","","0.1.0",[](const clap_host_t*,const char*)->const void*{return nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
  Instrument instrument(&host,RUSTIAS_PREVIEW_RESOURCES);if(!instrument.activate(48000))return 1;
  NSWindow* window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,1100,800) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];
  window.title=@"Rustias · silent plugin preview";[window center];instrument.editor=std::make_unique<Editor>(instrument);instrument.editor->parent((__bridge void*)window.contentView);instrument.editor->show(true);[window makeKeyAndOrderFront:nil];
  auto instance=&instrument;NSTimer* timer=[NSTimer scheduledTimerWithTimeInterval:256./48000 repeats:YES block:^(NSTimer*){
    float l[256],r[256];float* channels[]={l,r};clap_audio_buffer_t output{};output.channel_count=2;output.data32=channels;clap_process_t p{};p.frames_count=256;p.audio_outputs=&output;p.audio_outputs_count=1;instance->process(&p);
  }];
  [NSApp run];[timer invalidate];instrument.editor.reset();return 0;
}}
