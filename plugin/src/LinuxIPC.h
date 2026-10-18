#pragma once
#include <sys/socket.h>
#include <unistd.h>
#include <fcntl.h>
#include <cerrno>
#include <algorithm>
#include <string>
#include <vector>
// Newline-delimited JSON; strings escape internal newlines. Partial writes are
// retained so large patch/sample snapshots never block the DAW's GUI thread.
class LinuxIPC {
  int fd;
  std::string incoming,outgoing;
  size_t written=0;
public:
  bool connected=true;
  explicit LinuxIPC(int socket):fd(socket){fcntl(fd,F_SETFL,fcntl(fd,F_GETFL)|O_NONBLOCK);fcntl(fd,F_SETFD,FD_CLOEXEC);}
  ~LinuxIPC(){close(fd);}
  void send(const std::string& line){outgoing+=line;outgoing+='\n';flush();}
  void flush(){
    for(unsigned n=0;n<64&&connected&&written<outgoing.size();n++){
      auto count=::send(fd,outgoing.data()+written,std::min<size_t>(32768,outgoing.size()-written),MSG_NOSIGNAL|MSG_DONTWAIT);
      if(count>0)written+=size_t(count);
      else if(count<0&&errno==EINTR)continue;
      else if(count<0&&(errno==EAGAIN||errno==EWOULDBLOCK))break;
      else {connected=false;break;}
    }
    if(written==outgoing.size()){outgoing.clear();written=0;}
  }
  std::vector<std::string> receive(){
    flush();std::vector<std::string> messages;char bytes[32768];
    for(unsigned n=0;n<64&&connected;n++){
      auto count=recv(fd,bytes,sizeof bytes,MSG_DONTWAIT);
      if(count>0)incoming.append(bytes,size_t(count));
      else if(count<0&&errno==EINTR)continue;
      else if(count<0&&(errno==EAGAIN||errno==EWOULDBLOCK))break;
      else {connected=false;break;}
    }
    size_t start=0;
    for(auto end=incoming.find('\n',start);end!=std::string::npos;end=incoming.find('\n',start)){messages.push_back(incoming.substr(start,end-start));start=end+1;}
    if(start)incoming.erase(0,start);
    return messages;
  }
};
