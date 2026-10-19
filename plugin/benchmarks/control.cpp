// Silent full-runtime control/recording benchmark. Counts C++ new calls only;
// Rust allocations are not included. Separate processes are used for releases.
#include "../tests/control-scenarios.h"
#include <algorithm>
#include <array>
#include <chrono>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <memory>
#include <new>
#include <vector>
using Json=nlohmann::json;
static thread_local bool counting=false;
static thread_local uint64_t allocations=0;
void* operator new(std::size_t size){if(counting)allocations++;if(void* p=std::malloc(size?size:1))return p;throw std::bad_alloc();}
void* operator new[](std::size_t size){return ::operator new(size);}
void operator delete(void* p)noexcept{std::free(p);}
void operator delete[](void* p)noexcept{std::free(p);}
void operator delete(void* p,std::size_t)noexcept{std::free(p);}
void operator delete[](void* p,std::size_t)noexcept{std::free(p);}
struct Scene{const char* name;unsigned instances;bool midi,editor,motion,record;};
int main(int argc,char** argv){try{
  if(argc!=3)throw std::runtime_error("Usage: control-benchmark RESOURCES RESULTS.json");
  using Clock=std::chrono::steady_clock;
  Json results=Json::array();
  const std::array scenes={Scene{"static-two",2,false,false,false,false},Scene{"midi-two-editor-off",2,true,false,false,false},Scene{"midi-two-editor-on",2,true,true,false,false},Scene{"midi-eight-editor-on",8,true,true,false,false},Scene{"fx-slide-macros-two",2,true,true,true,false},Scene{"record-two",2,false,false,false,true}};
  for(unsigned frames:{32u,128u}){
    std::array<std::vector<double>,scenes.size()> times;
    std::array<uint64_t,scenes.size()> calls{};
    for(unsigned round=0;round<3;round++)for(unsigned order=0;order<scenes.size();order++){
      unsigned j=round%2?scenes.size()-1-order:order;auto s=scenes[j];
      std::vector<std::unique_ptr<Runtime>> units;
      for(unsigned i=0;i<s.instances;i++){
        auto r=std::make_unique<Runtime>(argv[1],48000);
        if(s.motion)ControlScenario::configure(*r,0);
        else{for(auto p:{std::pair{17,127},{18,0},{19,0},{9,127},{3,0},{4,127},{5,127},{1,80},{2,45}})ControlScenario::control(*r,p.first,p.second);for(unsigned n=0;n<6;n++)r->midi(0x90,48+n*3,100);}
        r->telemetry(s.editor);units.push_back(std::move(r));
      }
      std::array<float,128> l{},r{};
      for(unsigned b=0;b<128;b++)for(auto& unit:units)unit->render(l.data(),r.data(),frames);
      for(auto& unit:units){unit->takeMessages();if(s.record)unit->message(R"({"type":"record-start","id":"benchmark"})");}
      for(unsigned b=0;b<1024;b++){
        auto begin=Clock::now();allocations=0;counting=true;
        for(auto& unit:units){if(s.midi){unit->midi(0xb0,1,b%128);unit->midi(0xe0,0,(b*3)%128);}if(s.motion&&b%16==0)unit->parameter(b/16%8,b%101);unit->render(l.data(),r.data(),frames);}
        counting=false;calls[j]+=allocations;times[j].push_back(std::chrono::duration<double,std::micro>(Clock::now()-begin).count());
        // Consumer work is deliberately outside callback timing/allocation scope.
        if(b%64==0)for(auto& unit:units)unit->takeMessages();
      }
      for(auto& unit:units){if(s.record)unit->message(R"({"type":"record-stop","id":"benchmark"})");unit->takeMessages();}
    }
    for(unsigned j=0;j<scenes.size();j++){
      auto t=times[j];std::sort(t.begin(),t.end());double sum=0;for(auto v:t)sum+=v;
      results.push_back({{"scene",scenes[j].name},{"rate",48000},{"frames",frames},{"mean_us",sum/t.size()},{"p99_us",t[size_t((t.size()-1)*.99)]},{"max_us",t.back()},{"over_budget",std::count_if(t.begin(),t.end(),[&](double v){return v>frames/48000.*1e6;})},{"cpp_new_calls",calls[j]},{"callbacks",t.size()}});
    }
  }
  std::ofstream file(argv[2]);file<<results.dump(2)<<'\n';
}catch(const std::exception& e){counting=false;std::cerr<<e.what()<<'\n';return 1;}}
