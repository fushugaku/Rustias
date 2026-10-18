#pragma once
#include <functional>
#include <memory>
#include <string>
#include <vector>
// UI-only state in the helper process. No DSP, Rust engine or audio device.
struct LinuxEditorState {
  std::string resources,initial,lastError;
  unsigned width=1100,height=800;
  std::function<void(const std::string&)> send;
  std::vector<std::string> messages;
  std::string initialState(){return initial;}
  void uiMessage(const std::string& message){send(message);}
  std::vector<std::string> pull(){std::vector<std::string> result;result.swap(messages);return result;}
};
class LinuxEditorView {
public:
  explicit LinuxEditorView(LinuxEditorState&);
  ~LinuxEditorView();
  bool parent(void*);
  void size(unsigned,unsigned);
  void show(bool);
  void poll();
  void restore();
private:
  struct Impl;
  std::unique_ptr<Impl> impl;
};
