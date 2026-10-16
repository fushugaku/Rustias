// UI validation host: renders into discarded memory, never opens an audio device.
#include "Instrument.h"
#include "Editor.h"
#import <Cocoa/Cocoa.h>
#include <cstring>
struct PreviewHost { NSWindow* window=nil;Instrument* instrument=nullptr; };
static const clap_host_gui_t previewGui={
  [](const clap_host_t*){},
  [](const clap_host_t* host,uint32_t w,uint32_t h){auto& context=*static_cast<PreviewHost*>(host->host_data);if(!context.window||!context.instrument)return false;[context.window setContentSize:NSMakeSize(w,h)];context.instrument->width=w;context.instrument->height=h;context.instrument->editor->size(w,h);return true;},
  [](const clap_host_t*){return false;},[](const clap_host_t*){return false;},[](const clap_host_t*,bool){}
};
int main(){@autoreleasepool{
  freopen(RUSTIAS_PREVIEW_RESOURCES "/../ui-errors.log","a",stderr);
  [NSApplication sharedApplication];[NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
  NSMenu* menu=[NSMenu new];NSMenuItem* appMenu=[NSMenuItem new];NSMenu* items=[NSMenu new];[items addItemWithTitle:@"Quit silent preview" action:@selector(terminate:) keyEquivalent:@"q"];appMenu.submenu=items;[menu addItem:appMenu];NSApp.mainMenu=menu;
  PreviewHost context;
  clap_host_t host={CLAP_VERSION,&context,"Silent validation host","Rustias","","0.1.0",[](const clap_host_t*,const char* id)->const void*{return !strcmp(id,CLAP_EXT_GUI)?&previewGui:nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
  Instrument instrument(&host,RUSTIAS_PREVIEW_RESOURCES);if(!instrument.activate(48000))return 1;
  NSWindow* window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,1100,800) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];
  context.window=window;context.instrument=&instrument;window.contentMinSize=NSMakeSize(640,480);window.contentMaxSize=NSMakeSize(3840,2160);
  window.title=@"Rustias · silent plugin preview";[window center];instrument.editor=std::make_unique<Editor>(instrument);instrument.editor->parent((__bridge void*)window.contentView);instrument.editor->show(true);[window makeKeyAndOrderFront:nil];
  auto instance=&instrument;NSTimer* timer=[NSTimer scheduledTimerWithTimeInterval:256./48000 repeats:YES block:^(NSTimer*){
    float l[256],r[256];float* channels[]={l,r};clap_audio_buffer_t output{};output.channel_count=2;output.data32=channels;clap_process_t p{};p.frames_count=256;p.audio_outputs=&output;p.audio_outputs_count=1;instance->process(&p);
  }];
  [NSApp run];[timer invalidate];instrument.editor.reset();return 0;
}}
