#include "Editor.h"
#include "Instrument.h"
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>
#import <CommonCrypto/CommonDigest.h>
@interface RustiasWebDelegate:NSObject<WKURLSchemeHandler,WKScriptMessageHandler,WKNavigationDelegate,WKUIDelegate>
@property(nonatomic,assign) Instrument* instrument;
@property(nonatomic,strong) NSString* root;
@property(nonatomic,weak) WKWebView* web;
@end
static NSString* string(const std::string& s){return [[NSString alloc] initWithBytes:s.data() length:s.size() encoding:NSUTF8StringEncoding];}
static NSString* encode(id v){return [[NSString alloc] initWithData:[NSJSONSerialization dataWithJSONObject:v options:NSJSONWritingFragmentsAllowed error:nil] encoding:NSUTF8StringEncoding];}
@implementation RustiasWebDelegate
-(void)webView:(WKWebView*)web startURLSchemeTask:(id<WKURLSchemeTask>)task {
  NSString* relative=task.request.URL.path.stringByRemovingPercentEncoding;if(!relative.length||[relative isEqual:@"/"])relative=@"index.html";
  NSString* path=[self.root stringByAppendingPathComponent:relative].stringByStandardizingPath;
  if(![path hasPrefix:[self.root stringByAppendingString:@"/"]]){[task didFailWithError:[NSError errorWithDomain:NSURLErrorDomain code:NSURLErrorNoPermissionsToReadFile userInfo:nil]];return;}
  NSData* data=[NSData dataWithContentsOfFile:path];if(!data){[task didFailWithError:[NSError errorWithDomain:NSURLErrorDomain code:NSURLErrorFileDoesNotExist userInfo:nil]];return;}
  NSString* mime=@{@"html":@"text/html",@"js":@"text/javascript",@"css":@"text/css",@"json":@"application/json",@"wasm":@"application/wasm",@"svg":@"image/svg+xml",@"wav":@"audio/wav"}[path.pathExtension.lowercaseString]?:@"application/octet-stream";
  auto response=[[NSHTTPURLResponse alloc] initWithURL:task.request.URL statusCode:200 HTTPVersion:@"HTTP/1.1" headerFields:@{@"Content-Type":mime,@"Access-Control-Allow-Origin":@"*"}];[task didReceiveResponse:response];[task didReceiveData:data];[task didFinish];
}
-(void)webView:(WKWebView*)web stopURLSchemeTask:(id<WKURLSchemeTask>)task {}
-(void)userContentController:(WKUserContentController*)controller didReceiveScriptMessage:(WKScriptMessage*)message {
  if(!self.instrument||![message.body isKindOfClass:NSString.class])return;NSString* body=message.body;
  NSDictionary* data=[NSJSONSerialization JSONObjectWithData:[body dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];if(![data isKindOfClass:NSDictionary.class])return;
  if([data[@"kind"] isEqual:@"diagnostic"]){self.instrument->lastError=[data[@"text"] UTF8String];fprintf(stderr,"Rustias interface: %s\n",self.instrument->lastError.c_str());}
  else if([data[@"kind"] isEqual:@"export"]){
    NSData* bytes=[[NSData alloc] initWithBase64EncodedString:data[@"bytes"] options:0];if(!bytes)return;
    NSSavePanel* panel=NSSavePanel.savePanel;panel.nameFieldStringValue=[data[@"name"] lastPathComponent]?:@"Rustias-export";
    [panel beginSheetModalForWindow:self.web.window completionHandler:^(NSModalResponse result){if(result==NSModalResponseOK)[bytes writeToURL:panel.URL options:NSDataWritingAtomic error:nil];}];
  }else if([data[@"kind"] isEqual:@"digest"]){
    NSData* bytes=[[NSData alloc] initWithBase64EncodedString:data[@"bytes"] options:0];if(!bytes)return;unsigned char digest[CC_SHA256_DIGEST_LENGTH];CC_SHA256(bytes.bytes,(CC_LONG)bytes.length,digest);NSMutableArray* values=[NSMutableArray array];for(auto b:digest)[values addObject:@(b)];
    NSString* reply=encode(@{@"type":@"digest",@"request":data[@"request"],@"bytes":values});[self.web evaluateJavaScript:[NSString stringWithFormat:@"window.__rustiasPluginReceive?.(%@)",reply] completionHandler:nil];
  }else self.instrument->uiMessage(body.UTF8String);
}
-(void)webView:(WKWebView*)web decidePolicyForNavigationAction:(WKNavigationAction*)action decisionHandler:(void (^)(WKNavigationActionPolicy))handler {
  NSURL* url=action.request.URL;if([url.scheme isEqual:@"rustias-plugin"]||[url.scheme isEqual:@"about"]){handler(WKNavigationActionPolicyAllow);return;}
  if(action.navigationType==WKNavigationTypeLinkActivated&&[url.scheme isEqual:@"https"])[NSWorkspace.sharedWorkspace openURL:url];handler(WKNavigationActionPolicyCancel);
}
-(void)webView:(WKWebView*)web runOpenPanelWithParameters:(WKOpenPanelParameters*)parameters initiatedByFrame:(WKFrameInfo*)frame completionHandler:(void (^)(NSArray<NSURL*>*))completion {
  NSOpenPanel* panel=NSOpenPanel.openPanel;panel.allowsMultipleSelection=parameters.allowsMultipleSelection;panel.canChooseDirectories=NO;
  [panel beginSheetModalForWindow:web.window completionHandler:^(NSModalResponse result){completion(result==NSModalResponseOK?panel.URLs:nil);}];
}
@end
struct Editor::Impl {
  Instrument& instrument;__strong WKWebView* web;__strong RustiasWebDelegate* delegate;__strong NSTimer* timer;
  Impl(Instrument& s):instrument(s){
    delegate=[RustiasWebDelegate new];delegate.instrument=&s;delegate.root=string(s.resources+"/web").stringByStandardizingPath;
    WKWebViewConfiguration* config=[WKWebViewConfiguration new];config.websiteDataStore=getenv("RUSTIAS_UI_CHECK")?WKWebsiteDataStore.nonPersistentDataStore:WKWebsiteDataStore.defaultDataStore;[config setURLSchemeHandler:delegate forURLScheme:@"rustias-plugin"];[config.userContentController addScriptMessageHandler:delegate name:@"rustiasHost"];
    NSString* initial=[[@"window.__RUSTIAS_PLUGIN__=" stringByAppendingString:string(s.initialState())] stringByAppendingString:@";window.addEventListener('error',e=>window.webkit.messageHandlers.rustiasHost.postMessage(JSON.stringify({kind:'diagnostic',text:e.message+' '+(e.filename??'')})));window.addEventListener('unhandledrejection',e=>window.webkit.messageHandlers.rustiasHost.postMessage(JSON.stringify({kind:'diagnostic',text:String(e.reason)}))); "];
    [config.userContentController addUserScript:[[WKUserScript alloc] initWithSource:initial injectionTime:WKUserScriptInjectionTimeAtDocumentStart forMainFrameOnly:YES]];
    web=[[WKWebView alloc] initWithFrame:NSMakeRect(0,0,s.width,s.height) configuration:config];delegate.web=web;web.navigationDelegate=delegate;web.UIDelegate=delegate;web.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;[web loadRequest:[NSURLRequest requestWithURL:[NSURL URLWithString:@"rustias-plugin://app/index.html?interface=new"]]];
  }
  ~Impl(){[timer invalidate];[web stopLoading];[web removeFromSuperview];[web.configuration.userContentController removeScriptMessageHandlerForName:@"rustiasHost"];delegate.instrument=nullptr;}
};
Editor::Editor(Instrument& s):impl(std::make_unique<Impl>(s)){auto owner=this;impl->timer=[NSTimer scheduledTimerWithTimeInterval:1./30 repeats:YES block:^(NSTimer*){owner->poll();}];}
Editor::~Editor()=default;
bool Editor::parent(void* parent){if(!parent)return false;auto p=(__bridge NSView*)parent;[impl->web removeFromSuperview];impl->web.frame=p.bounds;[p addSubview:impl->web];return true;}
void Editor::size(unsigned w,unsigned h){[impl->web setFrameSize:NSMakeSize(w,h)];}
void Editor::show(bool visible){impl->web.hidden=!visible;if(!visible)[impl->web removeFromSuperview];}
void Editor::poll(){auto messages=impl->instrument.pull();if(messages.empty())return;std::string script="[";for(auto& s:messages){if(script.size()>1)script+=",";script+=s;}script+="].forEach(data=>window.__rustiasPluginReceive?.(data))";[impl->web evaluateJavaScript:string(script) completionHandler:nil];}
void Editor::restore(){[impl->web evaluateJavaScript:[NSString stringWithFormat:@"window.__rustiasPluginRestore?.(%@)",string(impl->instrument.initialState())] completionHandler:nil];}
