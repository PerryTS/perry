#ifndef PERRY_VFS_H
#define PERRY_VFS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Fixed-width status values for Perry's supplemental C VFS API. */
typedef int32_t perry_vfs_status;
#define PERRY_VFS_OK ((perry_vfs_status)0)
#define PERRY_VFS_NOT_FOUND ((perry_vfs_status)1)
#define PERRY_VFS_INVALID_ARGUMENT ((perry_vfs_status)2)
#define PERRY_VFS_INVALID_UTF8 ((perry_vfs_status)3)

/*
 * Read an embedded VFS path such as "$perryfs/message.txt". Paths use the
 * same prefix, dot-segment, and separator normalization as Perry's fs reads.
 * Both output pointers are required. On error, each non-null output is reset
 * to NULL/0. A NULL path is valid only when path_len is zero; lengths greater
 * than PTRDIFF_MAX and invalid UTF-8 return INVALID_ARGUMENT and INVALID_UTF8.
 * A missing embedded path returns NOT_FOUND.
 *
 * On success, *data is immutable and remains valid until process exit. Do not
 * modify or free it. Raw embedded bytes are returned without copying;
 * compressed registry entries can be decoded once into process-lifetime
 * memory. This function never extracts files and does not intercept ordinary
 * operating-system file access.
 */
perry_vfs_status perry_vfs_read_file_v1(
    const uint8_t *path,
    size_t path_len,
    const uint8_t **data,
    size_t *data_len);

#ifdef __cplusplus
}
#endif
#endif
