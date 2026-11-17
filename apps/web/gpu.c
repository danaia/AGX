/* DMA-BUF metadata and fd delivery. The source stays owned until the GUI ACKs
 * completion of its GPU copy, so WebKit cannot recycle a buffer still in use. */
#include "host.h"
#include <errno.h>
#include <string.h>
#include <sys/socket.h>

gboolean web_gpu_publish(WebHost *host, WPEBuffer *buffer, int width, int height)
{
    WPEBufferDMABuf *dma = WPE_BUFFER_DMA_BUF(buffer);
    guint8 packet[44];
    memcpy(packet, "ANGIGPU1", 8);
    guint32 words[5] = { GUINT32_TO_BE(width), GUINT32_TO_BE(height),
        GUINT32_TO_BE(wpe_buffer_dma_buf_get_format(dma)),
        GUINT32_TO_BE(wpe_buffer_dma_buf_get_stride(dma, 0)),
        GUINT32_TO_BE(wpe_buffer_dma_buf_get_offset(dma, 0)) };
    memcpy(packet + 8, words, sizeof(words));
    guint64 modifier = GUINT64_TO_BE(wpe_buffer_dma_buf_get_modifier(dma));
    guint64 sequence = GUINT64_TO_BE(host->sequence);
    memcpy(packet + 28, &modifier, 8); memcpy(packet + 36, &sequence, 8);
    int fd = wpe_buffer_dma_buf_get_fd(dma, 0);
    union { struct cmsghdr align; guint8 bytes[CMSG_SPACE(sizeof(int))]; } control = {0};
    struct iovec iov = { .iov_base = packet, .iov_len = sizeof(packet) };
    struct msghdr message = { .msg_iov = &iov, .msg_iovlen = 1,
        .msg_control = control.bytes, .msg_controllen = sizeof(control.bytes) };
    struct cmsghdr *header = CMSG_FIRSTHDR(&message);
    header->cmsg_level = SOL_SOCKET; header->cmsg_type = SCM_RIGHTS;
    header->cmsg_len = CMSG_LEN(sizeof(fd)); memcpy(CMSG_DATA(header), &fd, sizeof(fd));
    ssize_t size;
    do { size = sendmsg(host->gpu_fd, &message, MSG_NOSIGNAL); } while (size < 0 && errno == EINTR);
    if (size != sizeof(packet)) {
        host->failed = TRUE; g_main_loop_quit(host->loop); return FALSE;
    }
    return TRUE;
}
