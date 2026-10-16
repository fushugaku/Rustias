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
static WKWebView* editorWeb(NSView* view){
  WKWebView* web=nil;for(NSView* child in view.subviews)if([child isKindOfClass:WKWebView.class])web=(WKWebView*)child;
  if(!web||web.hidden||web.superview!=view||web.frame.size.width<=0||web.frame.size.height<=0)throw std::runtime_error("AU editor must attach a visible WebView on every opening");
  auto deadline=[NSDate dateWithTimeIntervalSinceNow:10];
  NSString* ready=@"!!document.querySelector('.plugin-workspace') && document.body.dataset.engine === 'ready'";
  while(![evaluate(web,ready) boolValue]&&deadline.timeIntervalSinceNow>0)[NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.02]];
  if(![evaluate(web,ready) boolValue])throw std::runtime_error("AU workspace initialized");
  return web;
}
static void render(AudioUnit unit){
  float left[256]{},right[256]{};struct {UInt32 count;AudioBuffer channels[2];} output{2,{{1,sizeof left,left},{1,sizeof right,right}}};
  AudioTimeStamp time{};time.mFlags=kAudioTimeStampSampleTimeValid;AudioUnitRenderActionFlags flags=0;
  if(AudioUnitRender(unit,&flags,&time,0,256,reinterpret_cast<AudioBufferList*>(&output)))throw std::runtime_error("Apply AU automation into discarded audio buffers");
}
static void checkAUView(const char* path){
  NSURL* url=[NSURL fileURLWithPath:[NSString stringWithUTF8String:path] isDirectory:YES];
  auto bundle=CFBundleCreate(nullptr,(__bridge CFURLRef)url);if(!bundle||!CFBundleLoadExecutable(bundle))throw std::runtime_error("Load AU editor bundle");
  auto factory=reinterpret_cast<AudioComponentFactoryFunction>(CFBundleGetFunctionPointerForName(bundle,CFSTR("wrapAsAUV2_inst0Factory")));
  AudioComponentDescription desc{'aumu','Rsts','Fshg',0,0};auto component=AudioComponentRegister(&desc,CFSTR("Rustias: offline editor check"),260,factory);AudioUnit unit=nullptr;
  if(!component||AudioComponentInstanceNew(component,&unit)||AudioUnitInitialize(unit))throw std::runtime_error("Create AU editor instance");
  AudioUnitCocoaViewInfo info{};UInt32 size=sizeof info;if(AudioUnitGetProperty(unit,kAudioUnitProperty_CocoaUI,kAudioUnitScope_Global,0,&info,&size))throw std::runtime_error("AU Cocoa view descriptor");
  NSBundle* viewBundle=[NSBundle bundleWithURL:(__bridge NSURL*)info.mCocoaAUViewBundleLocation];[viewBundle load];
  id<AUCocoaUIBase> creator=[NSClassFromString((__bridge NSString*)info.mCocoaAUViewClass[0]) new];NSView* view=[creator uiViewForAudioUnit:unit withSize:NSZeroSize];
  CFRelease(info.mCocoaAUViewBundleLocation);CFRelease(info.mCocoaAUViewClass[0]);
  if(!view)throw std::runtime_error("Create actual AU Cocoa editor");
  auto window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,1100,800) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];window.releasedWhenClosed=NO;window.contentView=view;
  WKWebView* web=editorWeb(view);
  [window setContentSize:NSMakeSize(940,680)];
  if([evaluate(web,@"innerWidth") intValue]!=940||[evaluate(web,@"innerHeight") intValue]!=680)throw std::runtime_error("Host resize reflows actual AU editor");
  evaluate(web,@"window.webkit.messageHandlers.rustiasHost.postMessage(JSON.stringify({kind:'editor-resize',width:1240,height:820}));true");
  if(view.frame.size.width!=1240||view.frame.size.height!=820||[evaluate(web,@"innerWidth") intValue]!=1240||[evaluate(web,@"innerHeight") intValue]!=820)throw std::runtime_error("Plugin resize reaches AU host view");
  [window setContentSize:view.frame.size];
  if(AudioUnitSetParameter(unit,8,kAudioUnitScope_Global,0,42,0))throw std::runtime_error("Set AU output before reopening");
  render(unit);
  for(int cycle=0;cycle<4;cycle++){
    if(AudioUnitUninitialize(unit)||AudioUnitInitialize(unit))throw std::runtime_error("Power cycle AU with its editor still open");
    if(editorWeb(view)!=web)throw std::runtime_error("AU audio activation must preserve its open editor");
    window.contentView=[NSView new];window.contentView=view;
    if(editorWeb(view)!=web)throw std::runtime_error("AU view can leave and rejoin its host window without losing its editor");
  }
  // Logic can destroy its Cocoa editor while switching a plug-in off, then ask
  // for a new view without calling the CLAP show callback.
  for(int cycle=0;cycle<8;cycle++){
    WKWebView* previous=web;window.contentView=[NSView new];
    if(!previous.hidden||previous.superview!=view)throw std::runtime_error("Detached AU view retains its hidden editor until release or replacement");
    if(AudioUnitUninitialize(unit)||AudioUnitInitialize(unit))throw std::runtime_error("Reactivate AU with its editor closed");
    view=[creator uiViewForAudioUnit:unit withSize:NSZeroSize];if(!view)throw std::runtime_error("Recreate actual AU Cocoa editor");
    if(previous.superview)throw std::runtime_error("Replacing the AU editor disconnects its previous host view");
    if(view.frame.size.width!=1240||view.frame.size.height!=820)throw std::runtime_error("AU reopen requests the previous editor size");
    [window setContentSize:view.frame.size];window.contentView=view;web=editorWeb(view);
    if(web==previous)throw std::runtime_error("AU GUI destroy must release the previous editor");
    AudioUnitParameterValue level=0;
    if(AudioUnitGetParameter(unit,8,kAudioUnitScope_Global,0,&level)||level!=42||[evaluate(web,@"window.__RUSTIAS_PLUGIN__.parameters[8]") intValue]!=42||[evaluate(web,@"Number(document.querySelector('#volume').value)") intValue]!=42)throw std::runtime_error("AU reopen preserves native parameters and initializes the new editor from them");
    if([evaluate(web,@"innerWidth") intValue]!=1240||[evaluate(web,@"innerHeight") intValue]!=820)throw std::runtime_error("AU reopen preserves the editor size");
  }
  window.contentView=[NSView new];[window close];AudioUnitUninitialize(unit);AudioComponentInstanceDispose(unit);CFRelease(bundle);
  std::cout<<"Actual AU editor: host/plugin resize, 4 power/detach/reattach cycles and 8 editor replacements preserve visible content, automation and size.\n";
}
int main(int argc,char** argv){@autoreleasepool{
  if(argc!=2)return 1;setenv("RUSTIAS_UI_CHECK","1",1);[NSApplication sharedApplication];
  try{checkAUView(argv[1]);}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}return 0;
}}
