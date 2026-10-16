// Actual AU editor integration; no audio device or visible host window.
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>
#import <AudioUnit/AUCocoaUIView.h>
#include <AudioToolbox/AudioToolbox.h>
#include <iostream>
#include <stdexcept>
static id evaluate(WKWebView* web,NSString* script){
  __block bool done=false;__block id result=nil;__block NSError* failure=nil;
  [web evaluateJavaScript:script completionHandler:^(id value,NSError* error){result=value;failure=error;done=true;}];
  auto deadline=[NSDate dateWithTimeIntervalSinceNow:3];while(!done&&deadline.timeIntervalSinceNow>0)[NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
  if(!done||failure)throw std::runtime_error("AU editor JavaScript failed");return result;
}
static void checkAUView(const char* path){
  NSURL* url=[NSURL fileURLWithPath:[NSString stringWithUTF8String:path] isDirectory:YES];
  auto bundle=CFBundleCreate(nullptr,(__bridge CFURLRef)url);if(!bundle||!CFBundleLoadExecutable(bundle))throw std::runtime_error("Load AU editor bundle");
  auto factory=reinterpret_cast<AudioComponentFactoryFunction>(CFBundleGetFunctionPointerForName(bundle,CFSTR("wrapAsAUV2_inst0Factory")));
  AudioComponentDescription desc{'aumu','Rsts','Fshg',0,0};auto component=AudioComponentRegister(&desc,CFSTR("Rustias: offline editor check"),259,factory);AudioUnit unit=nullptr;
  if(!component||AudioComponentInstanceNew(component,&unit)||AudioUnitInitialize(unit))throw std::runtime_error("Create AU editor instance");
  AudioUnitCocoaViewInfo info{};UInt32 size=sizeof info;if(AudioUnitGetProperty(unit,kAudioUnitProperty_CocoaUI,kAudioUnitScope_Global,0,&info,&size))throw std::runtime_error("AU Cocoa view descriptor");
  NSBundle* viewBundle=[NSBundle bundleWithURL:(__bridge NSURL*)info.mCocoaAUViewBundleLocation];[viewBundle load];
  id<AUCocoaUIBase> creator=[NSClassFromString((__bridge NSString*)info.mCocoaAUViewClass[0]) new];NSView* view=[creator uiViewForAudioUnit:unit withSize:NSZeroSize];
  CFRelease(info.mCocoaAUViewBundleLocation);CFRelease(info.mCocoaAUViewClass[0]);
  if(!view)throw std::runtime_error("Create actual AU Cocoa editor");
  auto window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,1100,800) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];window.releasedWhenClosed=NO;window.contentView=view;
  WKWebView* web=nil;for(NSView* child in view.subviews)if([child isKindOfClass:WKWebView.class])web=(WKWebView*)child;
  if(!web)throw std::runtime_error("AU WebView attached to its host view");
  auto deadline=[NSDate dateWithTimeIntervalSinceNow:10];
  while(![evaluate(web,@"!!document.querySelector('.plugin-workspace')") boolValue]&&deadline.timeIntervalSinceNow>0)[NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.02]];
  if(![evaluate(web,@"!!document.querySelector('.plugin-workspace')") boolValue])throw std::runtime_error("AU workspace initialized");
  [window setContentSize:NSMakeSize(940,680)];
  if([evaluate(web,@"innerWidth") intValue]!=940||[evaluate(web,@"innerHeight") intValue]!=680)throw std::runtime_error("Host resize reflows actual AU editor");
  evaluate(web,@"window.webkit.messageHandlers.rustiasHost.postMessage(JSON.stringify({kind:'editor-resize',width:1240,height:820}));true");
  if(view.frame.size.width!=1240||view.frame.size.height!=820||[evaluate(web,@"innerWidth") intValue]!=1240||[evaluate(web,@"innerHeight") intValue]!=820)throw std::runtime_error("Plugin resize reaches AU host view");
  window.contentView=[NSView new];[window close];AudioUnitUninitialize(unit);AudioComponentInstanceDispose(unit);CFRelease(bundle);
  std::cout<<"Actual AU editor: host resize and plugin resize both reflow the full workspace.\n";
}
int main(int argc,char** argv){@autoreleasepool{
  if(argc!=2)return 1;setenv("RUSTIAS_UI_CHECK","1",1);[NSApplication sharedApplication];
  try{checkAUView(argv[1]);}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}return 0;
}}
