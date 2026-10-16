#include "Runtime.h"
#include <cmath>
#include <iostream>
#include <stdexcept>
#include <vector>
#include <algorithm>
static void require(bool b,const char* m){if(!b)throw std::runtime_error(m);}
int main(int argc,char** argv){
  try{
    require(argc==2,"Resource directory required");
    for(double rate:{48000.,44100.,96000.}){
      Runtime a(argv[1],rate),b(argv[1],rate);float l[257],r[257];
      a.message("{\"type\":\"control\",\"timbre\":0,\"parameter\":9,\"value\":127}");a.midi(0x90,69,100);
      float peak=0;for(unsigned block=0;block<100;block++){a.render(l,r,257);for(auto x:l){require(std::isfinite(x),"Finite audio");peak=std::max(peak,std::abs(x));}}
      if(peak<=.001f){std::cerr<<a.stats()<<"\n";for(auto& message:a.takeMessages())std::cerr<<message<<"\n";}
      require(peak>.001f,"Host MIDI produces stereo audio");
      b.render(l,r,257);require(std::all_of(l,l+257,[](float v){return v==0;}),"Plugin instances are isolated");
      a.midi(0x80,69,0);a.message("{\"type\":\"stop\"}");
      require(a.stats().find("\"capacity\":128")!=std::string::npos,"Same expanded 128-voice engine");
      std::cout<<"Shared WASM/worklet: "<<rate<<" Hz, MIDI, variable blocks and instance isolation passed; peak "<<peak<<"\n";
    }
  }catch(const std::exception& e){std::cerr<<e.what()<<"\n";return 1;}
}
