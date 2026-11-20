/*
 * Plain-C consumer of the Perry VFS API. Compile this file to a C object and
 * include that object in the Perry host's final native link. Embed a file
 * named message.txt with Perry's existing `--embed` option.
 */
#include <perry_vfs.h>

#include <stdio.h>

int perry_read_embedded_message(void) {
  static const uint8_t path[] = "$perryfs/message.txt";
  const uint8_t *data = NULL;
  size_t data_len = 0;
  perry_vfs_status status = perry_vfs_read_file_v1(
      path, sizeof(path) - 1, &data, &data_len);
  if (status != PERRY_VFS_OK) {
    fprintf(stderr, "embedded file lookup failed: %d\n", status);
    return 1;
  }
  if (fwrite(data, 1, data_len, stdout) != data_len) {
    perror("write embedded file");
    return 1;
  }
  return 0;
}
