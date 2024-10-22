#include <sys/socket.h>
#include <sys/un.h>
#include <stdio.h>
#include <unistd.h>
#include <stdlib.h>
#include <string.h>

int my_create_unix_socket() {
   int sockfd;
   struct sockaddr_un addr;

//    unlink("/tmp/mylog.sock");

   // 创建 Unix Domain 套接字
   sockfd = socket(AF_UNIX, SOCK_STREAM, 0);
   if (sockfd == -1) {
       perror("socket");
       return -1;
   }

   // 设置套接字地址结构
   addr.sun_family = AF_UNIX;
   strcpy(addr.sun_path, "/tmp/mylog.sock");


   // 绑定套接字
   if (bind(sockfd, (struct sockaddr*)&addr, sizeof(addr)) == -1) {
       perror("bind");
       close(sockfd);
       return -1;
   }

   return sockfd;
}

void my_log_writer(const char *message) {
   // 将日志消息写入自定义日志文件
   FILE *logFile = fopen("/var/log/my_simple_imuxsock.log", "a");
   if (logFile != NULL) {
       fprintf(logFile, "Received log: %s\n", message);
       fclose(logFile);
   } else {
       perror("fopen");
   }
}

int main() {
    int sockfd;
    char buffer[1024];

   struct sockaddr_un addr;

//    unlink("/tmp/mylog.sock");

   sockfd = my_create_unix_socket();
   if (sockfd == -1) {
       exit(EXIT_FAILURE);
   }

   // 简单的日志接收循环
   while (1) {
       ssize_t numBytes = recv(sockfd, buffer, sizeof(buffer) - 1, 0);
       if (numBytes > 0) {
           buffer[numBytes] = '\0'; // null-terminate the string
           printf("Received log: %s\n", buffer);
           my_log_writer(buffer);   // 写入日志文件
       }
   }

   close(sockfd);
    return 0;
}