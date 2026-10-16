#include "Editor.h"
#include "Instrument.h"
#include <windows.h>
#include <wrl.h>
#include <WebView2.h>
#include <shlobj.h>
#include <commdlg.h>
#include <nlohmann/json.hpp>
#include <filesystem>
#include <fstream>
#include <vector>
using Microsoft::WRL::ComPtr;
using Microsoft::WRL::Callback;
using Json=nlohmann::json;
static std::wstring wide(const std::string& text){int n=MultiByteToWideChar(CP_UTF8,0,text.data(),int(text.size()),nullptr,0);std::wstring out(n,L'\0');MultiByteToWideChar(CP_UTF8,0,text.data(),int(text.size()),out.data(),n);return out;}
static std::string utf8(const wchar_t* text){int n=WideCharToMultiByte(CP_UTF8,0,text,-1,nullptr,0,nullptr,nullptr);std::string out(n,0);WideCharToMultiByte(CP_UTF8,0,text,-1,out.data(),n,nullptr,nullptr);if(!out.empty())out.pop_back();return out;}
struct Editor::Impl {
  struct Life {Impl* ptr;};
  Instrument& instrument;
  HWND parentWindow=nullptr,timerWindow=nullptr;
  bool visible=false,creating=false;HRESULT comStatus;
  unsigned width,height;
  std::shared_ptr<Life> life;
  ComPtr<ICoreWebView2Controller> controller;
  ComPtr<ICoreWebView2> web;
  std::wstring profile;
  Impl(Instrument& s):instrument(s),width(s.width),height(s.height),life(std::make_shared<Life>(Life{this})){
    comStatus=CoInitializeEx(nullptr,COINIT_APARTMENTTHREADED);
    wchar_t* local=nullptr;SHGetKnownFolderPath(FOLDERID_LocalAppData,0,nullptr,&local);
    if(local){profile=(std::filesystem::path(local)/L"Rustias"/L"Plugin"/L"WebView2").wstring();CoTaskMemFree(local);std::filesystem::create_directories(profile);}
    static const wchar_t* name=L"RustiasPluginEvents";
    WNDCLASSW cls{};cls.lpfnWndProc=[](HWND window,UINT message,WPARAM w,LPARAM l)->LRESULT{
      auto self=reinterpret_cast<Impl*>(GetWindowLongPtrW(window,GWLP_USERDATA));
      if(message==WM_TIMER&&self){self->poll();return 0;}return DefWindowProcW(window,message,w,l);
    };cls.hInstance=GetModuleHandleW(nullptr);cls.lpszClassName=name;RegisterClassW(&cls);
    timerWindow=CreateWindowExW(0,name,L"",0,0,0,0,0,HWND_MESSAGE,nullptr,cls.hInstance,nullptr);
    SetWindowLongPtrW(timerWindow,GWLP_USERDATA,reinterpret_cast<LONG_PTR>(this));SetTimer(timerWindow,1,33,nullptr);
  }
  ~Impl(){life->ptr=nullptr;if(timerWindow){KillTimer(timerWindow,1);DestroyWindow(timerWindow);}if(controller)controller->Close();web.Reset();controller.Reset();if(SUCCEEDED(comStatus))CoUninitialize();}
  void eval(const std::string& source){if(web)web->ExecuteScript(wide(source).c_str(),nullptr);}
  void poll(){if(!web)return;auto messages=instrument.pull();if(messages.empty())return;std::string source="[";for(auto& message:messages){if(source.size()>1)source+=",";source+=message;}eval(source+"].forEach(data=>window.__rustiasPluginReceive?.(data))");}
  void size(){if(controller){RECT bounds{0,0,LONG(width),LONG(height)};controller->put_Bounds(bounds);}}
  void message(const std::string& text){
    auto d=Json::parse(text,nullptr,false);if(!d.is_object())return;
    if(d.value("kind",std::string())=="diagnostic"){instrument.lastError=d.value("text",std::string());OutputDebugStringW(wide(instrument.lastError).c_str());}
    else if(d.value("kind",std::string())=="export"){
      auto name=std::filesystem::u8path(d.value("name",std::string("Rustias-export"))).filename().wstring();wchar_t file[32768]{};wcsncpy_s(file,name.c_str(),_TRUNCATE);
      OPENFILENAMEW dialog{};dialog.lStructSize=sizeof(dialog);dialog.hwndOwner=parentWindow;dialog.lpstrFile=file;dialog.nMaxFile=32768;dialog.Flags=OFN_OVERWRITEPROMPT|OFN_PATHMUSTEXIST|OFN_NOCHANGEDIR;
      if(GetSaveFileNameW(&dialog)){
        const auto encoded=d.value("bytes",std::string());std::vector<unsigned char> bytes;bytes.reserve(encoded.size()*3/4);unsigned buffer=0,bits=0;
        for(auto c:encoded){const char* alphabet="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";auto p=strchr(alphabet,c);if(!p||c=='=')break;buffer=(buffer<<6)|unsigned(p-alphabet);bits+=6;if(bits>=8){bits-=8;bytes.push_back(static_cast<unsigned char>(buffer>>bits));}}
        std::ofstream out(std::filesystem::path(file),std::ios::binary);out.write(reinterpret_cast<const char*>(bytes.data()),std::streamsize(bytes.size()));
      }
    }else instrument.uiMessage(text);
  }
  bool attach(HWND window){
    parentWindow=window;if(controller){controller->put_ParentWindow(window);size();controller->put_IsVisible(visible);return true;}if(creating)return true;creating=true;
    std::weak_ptr<Life> weak=life;
    HRESULT status=CreateCoreWebView2EnvironmentWithOptions(nullptr,profile.c_str(),nullptr,Callback<ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler>([weak](HRESULT result,ICoreWebView2Environment* environment)->HRESULT{
      auto token=weak.lock();if(!token||!token->ptr)return S_OK;auto self=token->ptr;
      if(FAILED(result)||!environment){self->instrument.lastError="Install Microsoft Edge WebView2 Runtime to open the Rustias editor.";self->creating=false;return S_OK;}
      return environment->CreateCoreWebView2Controller(self->parentWindow,Callback<ICoreWebView2CreateCoreWebView2ControllerCompletedHandler>([weak](HRESULT result,ICoreWebView2Controller* controller)->HRESULT{
        auto token=weak.lock();if(!token||!token->ptr)return S_OK;auto self=token->ptr;self->creating=false;
        if(FAILED(result)||!controller){self->instrument.lastError="Cannot create the WebView2 editor.";return S_OK;}
        self->controller=controller;controller->get_CoreWebView2(&self->web);self->size();controller->put_IsVisible(self->visible);
        ComPtr<ICoreWebView2_3> origin;if(SUCCEEDED(self->web.As(&origin)))origin->SetVirtualHostNameToFolderMapping(L"rustias.local",wide(self->instrument.resources+"/web").c_str(),COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS);
        const std::string injection="window.__RUSTIAS_PLUGIN__="+self->instrument.initialState()+";window.webkit={messageHandlers:{rustiasHost:{postMessage:value=>window.chrome.webview.postMessage(value)}}};window.addEventListener('error',e=>window.chrome.webview.postMessage(JSON.stringify({kind:'diagnostic',text:e.message})));window.addEventListener('unhandledrejection',e=>window.chrome.webview.postMessage(JSON.stringify({kind:'diagnostic',text:String(e.reason)})));";
        self->web->AddScriptToExecuteOnDocumentCreated(wide(injection).c_str(),nullptr);
        EventRegistrationToken tokenMessage{};self->web->add_WebMessageReceived(Callback<ICoreWebView2WebMessageReceivedEventHandler>([weak](ICoreWebView2*,ICoreWebView2WebMessageReceivedEventArgs* args)->HRESULT{
          wchar_t* raw=nullptr;if(SUCCEEDED(args->TryGetWebMessageAsString(&raw))){auto token=weak.lock();if(token&&token->ptr)token->ptr->message(utf8(raw));CoTaskMemFree(raw);}return S_OK;
        }).Get(),&tokenMessage);
        EventRegistrationToken tokenNavigation{};self->web->add_NavigationStarting(Callback<ICoreWebView2NavigationStartingEventHandler>([](ICoreWebView2*,ICoreWebView2NavigationStartingEventArgs* args)->HRESULT{
          wchar_t* uri=nullptr;args->get_Uri(&uri);std::wstring url=uri?uri:L"";CoTaskMemFree(uri);if(url.rfind(L"https://rustias.local/",0)!=0&&url!=L"about:blank")args->put_Cancel(TRUE);return S_OK;
        }).Get(),&tokenNavigation);
        self->web->Navigate(L"https://rustias.local/index.html?interface=new");return S_OK;
      }).Get());
    }).Get());
    if(FAILED(status)){creating=false;instrument.lastError="WebView2 Runtime is unavailable.";return false;}return true;
  }
};
Editor::Editor(Instrument& s):impl(std::make_unique<Impl>(s)){}
Editor::~Editor()=default;
bool Editor::parent(void* parent){return parent&&impl->attach(static_cast<HWND>(parent));}
void Editor::size(unsigned w,unsigned h){impl->width=w;impl->height=h;impl->size();}
void Editor::show(bool visible){impl->visible=visible;if(impl->controller)impl->controller->put_IsVisible(visible);}
void Editor::poll(){impl->poll();}
void Editor::restore(){impl->eval("window.__rustiasPluginRestore?.("+impl->instrument.initialState()+")");}
