# BIRD as a device under test, with an SSH server.
#
# NOGGlass talks to a router over SSH and runs `birdc` there, so a BIRD node
# needs both. The routing daemon images that exist carry only the daemon.
FROM alpine:3.20

RUN apk add --no-cache bird openssh-server openssh-client iproute2 iputils \
 && ssh-keygen -A \
 && mkdir -p /run/bird /var/log \
 && adduser -D -s /bin/sh lg \
 && echo 'lg:looking-glass' | chpasswd \
 # birdc needs the control socket. Read-only is enforced by BIRD itself: this
 # account can run `show`, and `configure` fails without write access to the
 # socket's directory.
 && addgroup lg root

COPY bird.conf /etc/bird.conf
COPY start.sh /start.sh
RUN chmod +x /start.sh

EXPOSE 22
CMD ["/start.sh"]
