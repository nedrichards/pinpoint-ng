#include <gtk/gtk.h>

typedef struct
{
  guint64 samples;
  guint64 differing_pixels;
  guint64 channel_error;
  guint max_channel_error;
  guint min_x;
  guint min_y;
  guint max_x;
  guint max_y;
} Difference;

static GdkTexture *
load_png (const char *path)
{
  g_autoptr (GFile) file = g_file_new_for_path (path);
  g_autoptr (GError) error = NULL;
  GdkTexture *texture = gdk_texture_new_from_file (file, &error);

  if (texture == NULL)
    g_error ("Unable to load %s: %s", path, error->message);
  return texture;
}

int
main (int   argc,
      char *argv[])
{
  g_autoptr (GdkTexture) c_texture = NULL;
  g_autoptr (GdkTexture) rust_texture = NULL;
  g_autoptr (GBytes) c_bytes = NULL;
  g_autoptr (GBytes) rust_bytes = NULL;
  GdkTextureDownloader *c_downloader;
  GdkTextureDownloader *rust_downloader;
  const guint8 *c_pixels;
  const guint8 *rust_pixels;
  gsize c_stride;
  gsize rust_stride;
  Difference difference = { 0 };
  double mean_error;
  double differing_percent;

  if (argc != 3)
    {
      g_printerr ("usage: %s C.png RUST.png\n", argv[0]);
      return 2;
    }
  difference.min_x = G_MAXUINT;
  difference.min_y = G_MAXUINT;
  c_texture = load_png (argv[1]);
  rust_texture = load_png (argv[2]);
  if (gdk_texture_get_width (c_texture) != gdk_texture_get_width (rust_texture) ||
      gdk_texture_get_height (c_texture) != gdk_texture_get_height (rust_texture))
    g_error ("capture dimensions differ: C=%dx%d Rust=%dx%d",
             gdk_texture_get_width (c_texture),
             gdk_texture_get_height (c_texture),
             gdk_texture_get_width (rust_texture),
             gdk_texture_get_height (rust_texture));
  c_downloader = gdk_texture_downloader_new (c_texture);
  rust_downloader = gdk_texture_downloader_new (rust_texture);
  gdk_texture_downloader_set_format (c_downloader, GDK_MEMORY_R8G8B8A8_PREMULTIPLIED);
  gdk_texture_downloader_set_format (rust_downloader, GDK_MEMORY_R8G8B8A8_PREMULTIPLIED);
  c_bytes = gdk_texture_downloader_download_bytes (c_downloader, &c_stride);
  rust_bytes = gdk_texture_downloader_download_bytes (rust_downloader, &rust_stride);
  gdk_texture_downloader_free (c_downloader);
  gdk_texture_downloader_free (rust_downloader);
  c_pixels = g_bytes_get_data (c_bytes, NULL);
  rust_pixels = g_bytes_get_data (rust_bytes, NULL);
  for (int y = 0; y < gdk_texture_get_height (c_texture); y++)
    for (int x = 0; x < gdk_texture_get_width (c_texture); x++)
      {
        gboolean pixel_differs = FALSE;

        for (int channel = 0; channel < 4; channel++)
          {
            guint c_value = c_pixels[(gsize) y * c_stride + (gsize) x * 4 + channel];
            guint rust_value = rust_pixels[(gsize) y * rust_stride + (gsize) x * 4 + channel];
            guint error = ABS ((int) c_value - (int) rust_value);

            difference.samples++;
            difference.channel_error += error;
            difference.max_channel_error = MAX (difference.max_channel_error, error);
            pixel_differs |= error > 3;
          }
        difference.differing_pixels += pixel_differs;
        if (pixel_differs)
          {
            difference.min_x = MIN (difference.min_x, (guint) x);
            difference.min_y = MIN (difference.min_y, (guint) y);
            difference.max_x = MAX (difference.max_x, (guint) x);
            difference.max_y = MAX (difference.max_y, (guint) y);
          }
      }
  mean_error = (double) difference.channel_error / difference.samples;
  differing_percent = 100.0 * difference.differing_pixels /
                       ((double) gdk_texture_get_width (c_texture) *
                        gdk_texture_get_height (c_texture));
  if (difference.differing_pixels == 0)
    g_print ("CURL DIFF mean_error=%.4f max_channel_error=%u differing_percent=%.4f bbox=none\n",
             mean_error,
             difference.max_channel_error,
             differing_percent);
  else
    g_print ("CURL DIFF mean_error=%.4f max_channel_error=%u differing_percent=%.4f bbox=%u,%u-%u,%u\n",
             mean_error,
             difference.max_channel_error,
             differing_percent,
             difference.min_x,
             difference.min_y,
             difference.max_x,
             difference.max_y);
  return mean_error <= 1.0 && difference.max_channel_error <= 8 && differing_percent <= 1.0
           ? 0
           : 1;
}
