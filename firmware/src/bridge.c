#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/sys/byteorder.h>
#include <string.h>

#include "bridge.h"
#include "bridge_proto.h"

/*
 * UART1 uses the async (DMA) API: at 1 Mbaud a byte arrives every 10 us,
 * which the one-byte interrupt-driven mode can't keep up with while the
 * radio ISRs run. Bytes land in DMA buffers and are parsed in the callback.
 */
#define BR_NODE DT_NODELABEL(uart1)
#define BR_BUF_LEN 64
#define BR_RX_TIMEOUT_US 100

struct br_msg {
	uint8_t type;
	uint8_t len;
	uint8_t p[JC_BR_MAX_PAYLOAD];
};

K_MSGQ_DEFINE(br_q, sizeof(struct br_msg), 32, 4);

static const struct device *const br_uart = DEVICE_DT_GET(BR_NODE);
static uint8_t br_buf[2][BR_BUF_LEN];
static uint8_t br_next;

static uint8_t br_f[JC_BR_MAX_PAYLOAD + JC_BR_OVERHEAD];
static uint8_t br_n;

static atomic_t br_good;
static atomic_t br_bad;
static atomic_t br_devs;

uint8_t jc_bridge_devices(void)
{
	return (uint8_t)atomic_get(&br_devs);
}

uint32_t jc_bridge_frames(void)
{
	return (uint32_t)atomic_get(&br_good);
}

uint32_t jc_bridge_bad_frames(void)
{
	return (uint32_t)atomic_get(&br_bad);
}

static void br_feed(const uint8_t *d, size_t n)
{
	for (size_t i = 0; i < n; i++) {
		uint8_t c = d[i];

		if (br_n == 0) {
			if (c == JC_BR_SYNC) {
				br_f[br_n++] = c;
			}
			continue;
		}
		if (br_n == 2 && c > JC_BR_MAX_PAYLOAD) {
			/* impossible length: resync, maybe on this byte */
			br_n = 0;
			atomic_inc(&br_bad);
			if (c == JC_BR_SYNC) {
				br_f[br_n++] = c;
			}
			continue;
		}
		br_f[br_n++] = c;
		if (br_n < 3 || br_n < (size_t)br_f[2] + JC_BR_OVERHEAD) {
			continue;
		}

		uint8_t len = br_f[2];

		br_n = 0;
		if (jc_br_crc8(&br_f[1], (size_t)len + 2) != br_f[3 + len]) {
			atomic_inc(&br_bad);
			continue;
		}

		struct br_msg m = { .type = br_f[1], .len = len };

		memcpy(m.p, &br_f[3], len);
		if (k_msgq_put(&br_q, &m, K_NO_WAIT) == 0) {
			atomic_inc(&br_good);
		} else {
			atomic_inc(&br_bad);
		}
	}
}

static void br_rx_start(void);

static void br_restart_fn(struct k_work *w)
{
	ARG_UNUSED(w);
	br_rx_start();
}
static K_WORK_DELAYABLE_DEFINE(br_restart, br_restart_fn);

static void br_uart_cb(const struct device *dev, struct uart_event *evt,
		       void *user_data)
{
	ARG_UNUSED(user_data);

	switch (evt->type) {
	case UART_RX_RDY:
		br_feed(evt->data.rx.buf + evt->data.rx.offset, evt->data.rx.len);
		break;
	case UART_RX_BUF_REQUEST:
		if (uart_rx_buf_rsp(dev, br_buf[br_next], BR_BUF_LEN) == 0) {
			br_next ^= 1U;
		}
		break;
	case UART_RX_STOPPED:
		/* framing/overrun error; the frame in progress is garbage */
		br_n = 0;
		atomic_inc(&br_bad);
		break;
	case UART_RX_DISABLED:
		/* after an error the driver stops RX; bring it back */
		k_work_schedule(&br_restart, K_MSEC(5));
		break;
	default:
		break;
	}
}

static void br_rx_start(void)
{
	int err;

	br_n = 0;
	br_next = 1;
	err = uart_rx_enable(br_uart, br_buf[0], BR_BUF_LEN, BR_RX_TIMEOUT_US);
	if (err && err != -EBUSY) {
		k_work_schedule(&br_restart, K_MSEC(100));
	}
}

static void br_set_devices(uint8_t devs)
{
	uint8_t was = (uint8_t)atomic_set(&br_devs, devs);

	if (was != devs) {
		jc_bridge_changed(devs);
	}
}

static void br_dispatch(const struct br_msg *m)
{
	switch (m->type) {
	case JC_BR_KBD:
		if (m->len >= JC_BR_KBD_LEN) {
			jc_bridge_kbd(m->p[0], &m->p[1]);
		}
		break;
	case JC_BR_MOUSE:
		if (m->len >= JC_BR_MOUSE_LEN) {
			jc_bridge_mouse((int16_t)sys_get_le16(&m->p[1]),
					(int16_t)sys_get_le16(&m->p[3]),
					(int8_t)m->p[5], m->p[0]);
		}
		break;
	case JC_BR_STATUS:
		if (m->len >= JC_BR_STATUS_LEN && m->p[0] == JC_BR_PROTO_VER) {
			br_set_devices(m->p[1] & (JC_BR_HAS_KBD | JC_BR_HAS_MOUSE));
		}
		break;
	default:
		break;
	}
}

static void br_thread(void *a, void *b, void *c)
{
	int64_t last_status = 0;

	ARG_UNUSED(a); ARG_UNUSED(b); ARG_UNUSED(c);

	if (!device_is_ready(br_uart)) {
		return;
	}
	if (uart_callback_set(br_uart, br_uart_cb, NULL)) {
		return;
	}
	br_rx_start();

	while (1) {
		struct br_msg m;

		if (k_msgq_get(&br_q, &m, K_MSEC(JC_BR_STATUS_MS)) == 0) {
			if (m.type == JC_BR_STATUS) {
				last_status = k_uptime_get();
			}
			br_dispatch(&m);
		}
		if (atomic_get(&br_devs) &&
		    k_uptime_get() - last_status > JC_BR_LOST_MS) {
			br_set_devices(0);
		}
	}
}

/* Cooperative and just below the BT RX thread (CONFIG_BT_RX_PRIO = 8). */
K_THREAD_DEFINE(br_tid, 3072, br_thread, NULL, NULL, NULL,
		K_PRIO_COOP(9), 0, 0);
