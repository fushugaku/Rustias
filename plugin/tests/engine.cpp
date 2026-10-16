#include "NativeEngine.h"
#include <nlohmann/json.hpp>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <unordered_map>
#include <cmath>
#include <vector>
#include <thread>
#include <array>
using Json=nlohmann::json;
static void require(bool b,const std::string& message){if(!b)throw std::runtime_error(message);}
struct Engine {
  NativeEngine native;std::unordered_map<std::string,unsigned> names;
  Engine(){for(unsigned i=0;i<rustias_native_method_count();i++)names.emplace(rustias_native_method_name(i),i);}
  double call(const std::string& name,const std::vector<double>& args={}){double value=0;require(rustias_native_call(native.handle,names.at(name),args.data(),unsigned(args.size()),&value),"Native ABI: "+name);return value;}
  template<class T>void read(uint32_t token,std::vector<T>& values){require(rustias_native_read(native.handle,token,reinterpret_cast<uint8_t*>(values.data()),values.size()*sizeof(T)),"Native buffer read");}
  template<class T>void write(uint32_t token,const std::vector<T>& values){require(rustias_native_write(native.handle,token,reinterpret_cast<const uint8_t*>(values.data()),values.size()*sizeof(T)),"Native buffer write");}
  Json snapshot(){auto length=size_t(call("rustias_save"));auto token=uint32_t(call("rustias_preset_buffer"));std::vector<uint8_t> bytes(length);read(token,bytes);return Json::parse(bytes);}
};
int main(int argc,char** argv){try{
  require(argc==2,"Browser parity fixture required");std::ifstream input(argv[1]);auto fixtures=Json::parse(input);unsigned total=0;
  for(auto& scenario:fixtures.at("cases")){
    Engine e;uint32_t buffer=0;double worst=0;
    for(auto& op:scenario.at("operations")){
      if(op.contains("call")){auto value=e.call(op["call"],op["args"].get<std::vector<double>>());if(op.value("buffer",false))buffer=uint32_t(value);else if(op.contains("expected"))require(std::abs(value-op["expected"].get<double>())<1e-7,"Native result differs: "+op["call"].get<std::string>());}
      else if(op.contains("write"))e.write(buffer,op["write"].get<std::vector<uint8_t>>());
      else if(op.contains("floats"))e.write(buffer,op["floats"].get<std::vector<float>>());
      else if(op.contains("render")){
        const auto expected=op["expected"].get<std::vector<float>>();size_t at=0;double squares=0,maxError=0;std::vector<float> block(256);
        for(unsigned b=0;b<op["render"].get<unsigned>();b++){auto token=uint32_t(e.call("rustias_render"));e.read(token,block);for(auto value:block){require(std::isfinite(value),"Native PCM must be finite");auto delta=double(value)-expected.at(at++);squares+=delta*delta;maxError=std::max(maxError,std::abs(delta));}}
        auto rms=std::sqrt(squares/expected.size());worst=std::max(worst,maxError);
        require(maxError<.00005&&rms<.000005,"PCM parity: "+scenario["name"].get<std::string>()+" peak error="+std::to_string(maxError)+" RMS="+std::to_string(rms));
      }else if(op.contains("snapshot"))require(e.snapshot()==op["snapshot"],"Native state parity: "+scenario["name"].get<std::string>());
    }
    total++;std::cout<<scenario["name"].get<std::string>()<<": PCM/state passed (max error "<<worst<<")\n";
  }
  // The native ABI rejects stale/oversized writes and never shares instance state.
  Engine a,b;a.call("rustias_init");b.call("rustias_init");auto token=uint32_t(a.call("rustias_preset_buffer"));std::array<uint8_t,8> bytes{};
  require(!rustias_native_read(b.native.handle,token,bytes.data(),bytes.size()),"Buffer tokens belong to an instance");
  a.call("rustias_preset_reserve",{131072});require(!rustias_native_write(a.native.handle,token,bytes.data(),bytes.size()),"Resizing revokes old tokens");
  token=uint32_t(a.call("rustias_render"));require(!rustias_native_write(a.native.handle,token,bytes.data(),bytes.size()),"Render buffers are read-only");
  std::vector<uint8_t> tooLarge(1025);require(!rustias_native_read(a.native.handle,token,tooLarge.data(),tooLarge.size()),"Oversized buffer reads fail before copying");
  std::exception_ptr error;std::thread audio([&]{try{a.call("rustias_note",{0,69,100});for(int i=0;i<10;i++)a.call("rustias_render");}catch(...){error=std::current_exception();}});audio.join();if(error)std::rethrow_exception(error);
  require(a.call("rustias_voices")==1&&b.call("rustias_voices")==0,"Native instances stay isolated across thread migration");
  std::cout<<total<<" native/browser PCM and state scenarios passed. No audio output opened.\n";
}catch(const std::exception& e){std::cerr<<e.what()<<"\n";return 1;}}
