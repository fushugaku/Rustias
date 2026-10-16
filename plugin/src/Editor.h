#pragma once
#include <memory>
struct Instrument;
class Editor {
public:
  explicit Editor(Instrument&);
  ~Editor();
  bool parent(void*);
  void size(unsigned,unsigned);
  void show(bool);
  void poll();
  void restore();
private:
  struct Impl;
  std::unique_ptr<Impl> impl;
};
