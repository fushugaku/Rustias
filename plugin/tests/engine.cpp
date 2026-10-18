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
#include <bit>
using Json=nlohmann::json;
static void require(bool b,const std::string& message){if(!b)throw std::runtime_error(message);}
static bool equivalent(const Json& a,const Json& b){
  if(a.is_number()&&b.is_number())return std::abs(a.get<double>()-b.get<double>())<1e-7;
  if(a.is_object()&&b.is_object()){if(a.size()!=b.size())return false;for(auto it=a.begin();it!=a.end();++it)if(!b.contains(it.key())||!equivalent(it.value(),b[it.key()]))return false;return true;}
  if(a.is_array()&&b.is_array()){if(a.size()!=b.size())return false;for(size_t i=0;i<a.size();i++)if(!equivalent(a[i],b[i]))return false;return true;}
  return a==b;
}
static Json orderedModulation(Json status){std::sort(status["values"].begin(),status["values"].end(),[](const Json& a,const Json& b){return a["target"].dump()<b["target"].dump();});return status;}
struct Engine {
  NativeEngine native;std::unordered_map<std::string,unsigned> names;
  Engine(){for(unsigned i=0;i<rustias_native_method_count();i++)names.emplace(rustias_native_method_name(i),i);}
  double call(const std::string& name,const std::vector<double>& args={}){double value=0;require(rustias_native_call(native.handle,names.at(name),args.data(),unsigned(args.size()),&value),"Native ABI: "+name);return value;}
  template<class T>void read(uint32_t token,std::vector<T>& values){require(rustias_native_read(native.handle,token,reinterpret_cast<uint8_t*>(values.data()),values.size()*sizeof(T)),"Native buffer read");}
  template<class T>void write(uint32_t token,const std::vector<T>& values){require(rustias_native_write(native.handle,token,reinterpret_cast<const uint8_t*>(values.data()),values.size()*sizeof(T)),"Native buffer write");}
  Json snapshot(){auto length=size_t(call("rustias_save"));auto token=uint32_t(call("rustias_preset_buffer"));std::vector<uint8_t> bytes(length);read(token,bytes);return Json::parse(bytes);}
  Json modulation(){auto length=size_t(call("rustias_modulation_status"));auto token=uint32_t(call("rustias_preset_buffer"));std::vector<uint8_t> bytes(length);read(token,bytes);auto status=Json::parse(bytes);require(status["backend"]=="native-rust","Modulation runs in Rust");status.erase("backend");return status;}
};
int main(int argc,char** argv){try{
  require(argc==2,"Browser parity fixture required");std::ifstream input(argv[1]);auto fixtures=Json::parse(input);unsigned total=0;
  for(auto& scenario:fixtures.at("cases")){
    std::vector<float> original;size_t compared=0;
    for(bool streamed:{false,true}){
    std::exception_ptr scenarioError;std::thread audioThread([&]{try{
    std::cerr<<"Native scenario: "<<scenario["name"].get<std::string>()<<"\n";
    Engine e;uint32_t buffer=0;double worst=0;
    for(auto& op:scenario.at("operations")){
      if(op.contains("call")){auto value=e.call(op["call"],op["args"].get<std::vector<double>>());if(op.value("buffer",false))buffer=uint32_t(value);else if(op.contains("expected"))require(std::abs(value-op["expected"].get<double>())<1e-7,"Native result differs: "+op["call"].get<std::string>());}
      else if(op.contains("native")){auto value=e.call(op["native"],op["args"].get<std::vector<double>>());if(op.contains("expected"))require(std::abs(value-op["expected"].get<double>())<1e-7,"Native modulation result differs: "+op["native"].get<std::string>());}
      else if(op.contains("write"))e.write(buffer,op["write"].get<std::vector<uint8_t>>());
      else if(op.contains("floats"))e.write(buffer,op["floats"].get<std::vector<float>>());
      else if(op.contains("render")){
        const auto expected=op["expected"].get<std::vector<float>>();size_t at=0;double squares=0,maxError=0;std::vector<float> block(256);
        for(unsigned b=0;b<op["render"].get<unsigned>();b++){
          unsigned remaining=128;
          do{
            const unsigned count=streamed?std::min(remaining,std::array{1u,7u,17u,31u,32u,64u,113u,128u}[(b+remaining)%8]):128;
            auto token=uint32_t(streamed?e.call("rustias_render_frames",{double(count)}):e.call("rustias_render"));e.read(token,block);
            for(unsigned i=0;i<count*2;i++){
              auto value=block[i];require(std::isfinite(value),"Native PCM must be finite");
              if(streamed)require(std::bit_cast<uint32_t>(value)==std::bit_cast<uint32_t>(original.at(compared++)),"Splitting native blocks preserves every PCM bit");else original.push_back(value);
              auto delta=double(value)-expected.at(at++);squares+=delta*delta;maxError=std::max(maxError,std::abs(delta));
            }
            remaining-=count;
          }while(remaining);
        }
        auto rms=std::sqrt(squares/expected.size());worst=std::max(worst,maxError);
        require(maxError<.00005&&rms<.000005,"PCM parity: "+scenario["name"].get<std::string>()+" peak error="+std::to_string(maxError)+" RMS="+std::to_string(rms));
        if(op.contains("modulation")){const auto actual=orderedModulation(e.modulation()),expected=orderedModulation(op["modulation"]);if(!equivalent(actual,expected))std::cerr<<"Expected "<<expected.dump()<<"\nActual "<<actual.dump()<<"\n";require(equivalent(actual,expected),"Native modulation positions/values: "+scenario["name"].get<std::string>());}
      }else if(op.contains("snapshot"))require(e.snapshot()==op["snapshot"],"Native state parity: "+scenario["name"].get<std::string>());
    }
    total++;std::cout<<scenario["name"].get<std::string>()<<": PCM/state passed (max error "<<worst<<")\n";
    }catch(...){scenarioError=std::current_exception();}});audioThread.join();if(scenarioError)std::rethrow_exception(scenarioError);
    }
  }
  // The native ABI rejects stale/oversized writes and never shares instance state.
  Engine a,b;a.call("rustias_init");b.call("rustias_init");auto token=uint32_t(a.call("rustias_preset_buffer"));std::array<uint8_t,8> bytes{};
  for(double count:{-1.,0.,.5,129.}){double value=0;require(!rustias_native_call(a.native.handle,a.names.at("rustias_render_frames"),&count,1,&value),"Invalid native render lengths fail before processing");}
  require(!rustias_native_read(b.native.handle,token,bytes.data(),bytes.size()),"Buffer tokens belong to an instance");
  a.call("rustias_preset_reserve",{131072});require(!rustias_native_write(a.native.handle,token,bytes.data(),bytes.size()),"Resizing revokes old tokens");
  token=uint32_t(a.call("rustias_render"));require(!rustias_native_write(a.native.handle,token,bytes.data(),bytes.size()),"Render buffers are read-only");
  std::vector<uint8_t> tooLarge(1025);require(!rustias_native_read(a.native.handle,token,tooLarge.data(),tooLarge.size()),"Oversized buffer reads fail before copying");
  std::exception_ptr error;std::thread audio([&]{try{a.call("rustias_note",{0,69,100});for(int i=0;i<10;i++)a.call("rustias_render");}catch(...){error=std::current_exception();}});audio.join();if(error)std::rethrow_exception(error);
  require(a.call("rustias_voices")==1&&b.call("rustias_voices")==0,"Native instances stay isolated across thread migration");
  std::cout<<total<<" native/browser PCM and state scenarios passed. No audio output opened.\n";
}catch(const std::exception& e){std::cerr<<e.what()<<"\n";return 1;}}
