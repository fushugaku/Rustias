#include "LinuxEditor.h"
#include <gtk/gtk.h>
#include <gtk/gtkx.h>
#include <gdk/gdkx.h>
#include <webkit2/webkit2.h>
#include <libsoup/soup.h>
#include <nlohmann/json.hpp>
#include <filesystem>
#include <fstream>
#include <stdexcept>
using Json=nlohmann::json;
namespace fs=std::filesystem;

struct LinuxEditorView::Impl {
  LinuxEditorState& instrument;
  GtkWidget* plug=nullptr;
  WebKitWebView* web=nullptr;
  WebKitWebContext* context=nullptr;
  WebKitUserContentManager* manager=nullptr;
  guint timer=0;
  uintptr_t parentId=0;
  bool visible=false;
  unsigned width,height;
  explicit Impl(LinuxEditorState& s):instrument(s),width(s.width),height(s.height){
    if(!gtk_init_check(nullptr,nullptr))throw std::runtime_error("Rustias requires an X11/XWayland display.");
    if(!GDK_IS_X11_DISPLAY(gdk_display_get_default()))throw std::runtime_error("Run the DAW with GDK_BACKEND=x11 to embed Rustias through XWayland.");
    const bool testing=getenv("RUSTIAS_UI_CHECK");
    auto data=fs::path(g_get_user_data_dir())/"rustias/plugin/webkit";
    auto cache=fs::path(g_get_user_cache_dir())/"rustias/plugin/webkit";
    auto storage=testing?webkit_website_data_manager_new_ephemeral():webkit_website_data_manager_new("base-data-directory",data.c_str(),"base-cache-directory",cache.c_str(),nullptr);
    context=webkit_web_context_new_with_website_data_manager(storage);g_object_unref(storage);
    auto security=webkit_web_context_get_security_manager(context);
    webkit_security_manager_register_uri_scheme_as_secure(security,"rustias-plugin");
    webkit_security_manager_register_uri_scheme_as_cors_enabled(security,"rustias-plugin");
    auto root=new fs::path(fs::weakly_canonical(fs::path(s.resources)/"web"));
    webkit_web_context_register_uri_scheme(context,"rustias-plugin",[](WebKitURISchemeRequest* request,gpointer data){
      const auto& root=*static_cast<fs::path*>(data);
      auto raw=webkit_uri_scheme_request_get_path(request);auto decoded=g_uri_unescape_string(raw?raw:"/",nullptr);
      auto relative=fs::path(decoded?decoded:"/").relative_path();g_free(decoded);if(relative.empty())relative="index.html";
      std::error_code error;auto file=fs::weakly_canonical(root/relative,error);
      if(error||file.lexically_relative(root).empty()||*file.lexically_relative(root).begin()==".."){
        auto e=g_error_new_literal(G_IO_ERROR,G_IO_ERROR_PERMISSION_DENIED,"Invalid plugin resource path");webkit_uri_scheme_request_finish_error(request,e);g_error_free(e);return;
      }
      auto source=g_file_new_for_path(file.c_str());GError* e=nullptr;auto stream=g_file_read(source,nullptr,&e);g_object_unref(source);
      if(!stream){webkit_uri_scheme_request_finish_error(request,e);g_error_free(e);return;}
      const auto ext=file.extension().string();const char* mime=ext==".js"?"text/javascript":ext==".css"?"text/css":ext==".html"?"text/html":ext==".json"?"application/json":ext==".wasm"?"application/wasm":ext==".svg"?"image/svg+xml":ext==".wav"?"audio/wav":"application/octet-stream";
      auto response=webkit_uri_scheme_response_new(G_INPUT_STREAM(stream),-1);
      webkit_uri_scheme_response_set_content_type(response,mime);
      auto headers=soup_message_headers_new(SOUP_MESSAGE_HEADERS_RESPONSE);soup_message_headers_append(headers,"Access-Control-Allow-Origin","*");
      // transfer-full: WebKit owns and releases the headers after the response.
      webkit_uri_scheme_response_set_http_headers(response,headers);
      webkit_uri_scheme_request_finish_with_response(request,response);g_object_unref(response);g_object_unref(stream);
    },root,[](gpointer data){delete static_cast<fs::path*>(data);});
    manager=webkit_user_content_manager_new();
    g_signal_connect(manager,"script-message-received::rustiasHost",G_CALLBACK(+[](WebKitUserContentManager*,WebKitJavascriptResult* result,gpointer data){
      auto text=jsc_value_to_string(webkit_javascript_result_get_js_value(result));static_cast<Impl*>(data)->message(text?text:"");g_free(text);
    }),this);
    webkit_user_content_manager_register_script_message_handler(manager,"rustiasHost");
    const auto injection="window.__RUSTIAS_PLUGIN__="+s.initialState()+";window.addEventListener('error',e=>window.webkit.messageHandlers.rustiasHost.postMessage(JSON.stringify({kind:'diagnostic',text:e.message+' '+(e.error?.stack??e.filename??'')})));window.addEventListener('unhandledrejection',e=>window.webkit.messageHandlers.rustiasHost.postMessage(JSON.stringify({kind:'diagnostic',text:String(e.reason)+' '+(e.reason?.stack??'')})));";
    auto script=webkit_user_script_new(injection.c_str(),WEBKIT_USER_CONTENT_INJECT_TOP_FRAME,WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START,nullptr,nullptr);
    webkit_user_content_manager_add_script(manager,script);webkit_user_script_unref(script);
    web=WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW,"web-context",context,"user-content-manager",manager,nullptr));g_object_ref_sink(web);
    g_signal_connect(web,"decide-policy",G_CALLBACK(+[](WebKitWebView*,WebKitPolicyDecision* decision,WebKitPolicyDecisionType type,gpointer)->gboolean{
      if(type!=WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION)return FALSE;
      auto action=webkit_navigation_policy_decision_get_navigation_action(WEBKIT_NAVIGATION_POLICY_DECISION(decision));
      auto uri=webkit_uri_request_get_uri(webkit_navigation_action_get_request(action));
      if(g_str_has_prefix(uri,"rustias-plugin://app/")||!g_strcmp0(uri,"about:blank"))return FALSE;
      webkit_policy_decision_ignore(decision);return TRUE;
    }),nullptr);
    g_signal_connect(web,"load-failed",G_CALLBACK(+[](WebKitWebView*,WebKitLoadEvent,const gchar*,GError* e,gpointer data)->gboolean{static_cast<Impl*>(data)->instrument.lastError=e->message;return FALSE;}),this);
    webkit_web_view_load_uri(web,"rustias-plugin://app/index.html?interface=new");
    timer=g_timeout_add(33,[](gpointer data)->gboolean{static_cast<Impl*>(data)->deliver();return G_SOURCE_CONTINUE;},this);
  }
  ~Impl(){
    if(timer)g_source_remove(timer);
    if(web){webkit_web_view_stop_loading(web);g_signal_handlers_disconnect_by_data(web,this);gtk_widget_destroy(GTK_WIDGET(web));g_object_unref(web);}
    if(plug)gtk_widget_destroy(plug);
    if(manager){g_signal_handlers_disconnect_by_data(manager,this);webkit_user_content_manager_unregister_script_message_handler(manager,"rustiasHost");g_object_unref(manager);}
    if(context)g_object_unref(context);
  }
  void eval(const std::string& script){webkit_web_view_evaluate_javascript(web,script.c_str(),script.size(),nullptr,nullptr,nullptr,nullptr,nullptr);}
  void deliver(){auto messages=instrument.pull();if(messages.empty())return;std::string script="[";for(auto& message:messages){if(script.size()>1)script+=",";script+=message;}eval(script+"].forEach(data=>window.__rustiasPluginReceive?.(data))");}
  void message(const std::string& text){
    auto value=Json::parse(text,nullptr,false);if(!value.is_object())return;
    const auto kind=value.value("kind",std::string());
    if(kind=="diagnostic")instrument.lastError=value.value("text",std::string());
    else if(kind=="digest"){
      auto bytes=value.value("bytes",std::string());gsize length=0;auto decoded=g_base64_decode(bytes.c_str(),&length);
      auto checksum=g_checksum_new(G_CHECKSUM_SHA256);g_checksum_update(checksum,decoded,length);g_free(decoded);guint8 digest[32];gsize size=32;g_checksum_get_digest(checksum,digest,&size);g_checksum_free(checksum);
      eval("window.__rustiasPluginReceive?.("+Json{{"type","digest"},{"request",value["request"]},{"bytes",std::vector<unsigned>(digest,digest+32)}}.dump()+")");
    }else if(kind=="export"){
      auto dialog=gtk_file_chooser_dialog_new("Save recording",plug?GTK_WINDOW(plug):nullptr,GTK_FILE_CHOOSER_ACTION_SAVE,"Cancel",GTK_RESPONSE_CANCEL,"Save",GTK_RESPONSE_ACCEPT,nullptr);
      auto name=fs::path(value.value("name",std::string("Rustias-export"))).filename().string();gtk_file_chooser_set_current_name(GTK_FILE_CHOOSER(dialog),name.c_str());gtk_file_chooser_set_do_overwrite_confirmation(GTK_FILE_CHOOSER(dialog),TRUE);
      if(gtk_dialog_run(GTK_DIALOG(dialog))==GTK_RESPONSE_ACCEPT){auto file=gtk_file_chooser_get_filename(GTK_FILE_CHOOSER(dialog));gsize size=0;auto decoded=g_base64_decode(value.value("bytes",std::string()).c_str(),&size);GError* error=nullptr;if(!g_file_set_contents(file,reinterpret_cast<const char*>(decoded),size,&error)){instrument.lastError=error->message;g_error_free(error);}g_free(decoded);g_free(file);}
      gtk_widget_destroy(dialog);
    }else instrument.uiMessage(text);
  }
};
LinuxEditorView::LinuxEditorView(LinuxEditorState& instrument):impl(std::make_unique<Impl>(instrument)){}
LinuxEditorView::~LinuxEditorView()=default;
bool LinuxEditorView::parent(void* parent){
  const auto id=reinterpret_cast<uintptr_t>(parent);if(!id)return false;if(id==impl->parentId&&impl->plug)return true;
  if(impl->plug){gtk_container_remove(GTK_CONTAINER(impl->plug),GTK_WIDGET(impl->web));gtk_widget_destroy(impl->plug);}
  impl->plug=gtk_plug_new(id);impl->parentId=id;gtk_container_add(GTK_CONTAINER(impl->plug),GTK_WIDGET(impl->web));size(impl->width,impl->height);gtk_widget_show_all(impl->plug);show(impl->visible);return true;
}
void LinuxEditorView::size(unsigned width,unsigned height){impl->width=width;impl->height=height;if(impl->plug)gtk_window_resize(GTK_WINDOW(impl->plug),width,height);gtk_widget_set_size_request(GTK_WIDGET(impl->web),width,height);}
void LinuxEditorView::show(bool visible){impl->visible=visible;if(impl->plug){if(visible)gtk_widget_show_all(impl->plug);else gtk_widget_hide(impl->plug);}}
void LinuxEditorView::poll(){impl->deliver();}
void LinuxEditorView::restore(){impl->eval("window.__rustiasPluginRestore?.("+impl->instrument.initialState()+")");}
