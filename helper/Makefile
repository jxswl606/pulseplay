# PulsePlay helper (PS5 payload) — AGPL-3.0-only.
# Build: PS5_PAYLOAD_SDK=/opt/ps5-payload-sdk make
ifdef PS5_PAYLOAD_SDK
    include $(PS5_PAYLOAD_SDK)/toolchain/prospero.mk
else
    $(error PS5_PAYLOAD_SDK is undefined)
endif

ELF  := pulseplay-helper.elf
SRCS := src/main.c src/httpd.c src/util.c src/pt.c src/rp.c

CFLAGS += -O2 -Wall -Isrc
LDLIBS := -lkernel_sys -lSceRegMgr -lSceUserService -lpthread

all: $(ELF)

$(ELF): $(SRCS)
	$(CC) $(CFLAGS) -o $@ $(SRCS) $(LDLIBS)

clean:
	rm -f $(ELF)

# Convenience: send to a console running the ELF loader. Set PS5_IP.
send: $(ELF)
	socat -t 99999999 - TCP:$(PS5_IP):9021 < $(ELF)
