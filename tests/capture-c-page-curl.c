#include "config.h"
#include "pp-page-curl-view.h"

#include <gtk/gtk.h>

typedef struct
{
  GtkApplication *application;
  GtkWidget *view;
  const char *output;
} Capture;

static GdkTexture *
make_pattern (int      width,
              int      height,
              gboolean previous)
{
  g_autofree guint8 *pixels = NULL;
  g_autoptr (GBytes) bytes = NULL;
  gsize stride = (gsize) width * 4;

  pixels = g_malloc ((gsize) height * stride);
  for (int y = 0; y < height; y++)
    for (int x = 0; x < width; x++)
      {
        guint8 *pixel = pixels + (gsize) y * stride + (gsize) x * 4;
        guint8 checker = ((x / 32) ^ (y / 32)) & 1 ? 36 : 0;

        if (previous)
          {
            pixel[0] = (guint8) ((x * 255) / MAX (width - 1, 1));
            pixel[1] = (guint8) ((y * 255) / MAX (height - 1, 1));
            pixel[2] = (guint8) (60 + checker);
          }
        else
          {
            pixel[0] = (guint8) (28 + checker);
            pixel[1] = (guint8) ((x * 180) / MAX (width - 1, 1));
            pixel[2] = (guint8) (255 - (y * 180) / MAX (height - 1, 1));
          }
        pixel[3] = 255;
      }
  bytes = g_bytes_new_take (g_steal_pointer (&pixels), (gsize) height * stride);
  return gdk_memory_texture_new (width,
                                 height,
                                 GDK_MEMORY_R8G8B8A8_PREMULTIPLIED,
                                 bytes,
                                 stride);
}

static gboolean
capture_cb (gpointer data)
{
  Capture *capture = data;
  GtkNative *native = gtk_widget_get_native (capture->view);
  g_autoptr (GdkPaintable) paintable = NULL;
  g_autoptr (GtkSnapshot) snapshot = NULL;
  g_autoptr (GskRenderNode) node = NULL;
  g_autoptr (GdkTexture) texture = NULL;
  GskRenderer *renderer;

  if (native == NULL)
    g_error ("C page-curl capture view has no GtkNative");
  renderer = gtk_native_get_renderer (native);
  paintable = gtk_widget_paintable_new (capture->view);
  snapshot = gtk_snapshot_new ();
  gdk_paintable_snapshot (GDK_PAINTABLE (paintable),
                          GDK_SNAPSHOT (snapshot),
                          gtk_widget_get_width (capture->view),
                          gtk_widget_get_height (capture->view));
  node = gtk_snapshot_free_to_node (g_steal_pointer (&snapshot));
  if (node == NULL)
    g_error ("C page-curl capture snapshot was empty");
  texture = gsk_renderer_render_texture (renderer, node, NULL);
  if (!gdk_texture_save_to_png (texture, capture->output))
    g_error ("Unable to save C page-curl capture");
  g_print ("CURL CAPTURE C output=%s size=%dx%d\n",
           capture->output,
           gdk_texture_get_width (texture),
           gdk_texture_get_height (texture));
  g_application_quit (G_APPLICATION (capture->application));
  return G_SOURCE_REMOVE;
}

static void
activate_cb (GtkApplication *application,
             gpointer        user_data)
{
  Capture *capture = user_data;
  int width = GPOINTER_TO_INT (g_object_get_data (G_OBJECT (application), "capture-width"));
  int height = GPOINTER_TO_INT (g_object_get_data (G_OBJECT (application), "capture-height"));
  gboolean backwards = GPOINTER_TO_INT (g_object_get_data (G_OBJECT (application), "capture-backwards"));
  GtkWindow *window = GTK_WINDOW (gtk_application_window_new (application));
  g_autoptr (GdkTexture) previous = make_pattern (width, height, TRUE);
  g_autoptr (GdkTexture) current = make_pattern (width, height, FALSE);

  capture->application = application;
  capture->view = pp_page_curl_view_new ();
  gtk_window_set_decorated (window, FALSE);
  gtk_window_set_default_size (window, width, height);
  gtk_window_set_child (window, capture->view);
  pp_page_curl_view_set_transition (PP_PAGE_CURL_VIEW (capture->view),
                                    previous,
                                    current,
                                    backwards ? 0.0 : 0.5,
                                    0.0,
                                    backwards ? 0.5 : 0.0,
                                    0.0,
                                    backwards);
  gtk_window_present (window);
  g_timeout_add (150, capture_cb, capture);
}

int
main (int   argc,
      char *argv[])
{
  g_autoptr (GtkApplication) application = NULL;
  Capture capture = { 0 };
  guint64 parsed_width;
  guint64 parsed_height;
  gboolean backwards;

  if (argc != 5 ||
      !g_ascii_string_to_unsigned (argv[1], 10, 1, G_MAXINT, &parsed_width, NULL) ||
      !g_ascii_string_to_unsigned (argv[2], 10, 1, G_MAXINT, &parsed_height, NULL) ||
      (!g_str_equal (argv[3], "forward") && !g_str_equal (argv[3], "backward")))
    {
      g_printerr ("usage: %s WIDTH HEIGHT forward|backward OUTPUT.png\n", argv[0]);
      return 2;
    }
  backwards = g_str_equal (argv[3], "backward");
  capture.output = argv[4];
  application = gtk_application_new ("com.nedrichards.Pinpoint.CurlCapture",
                                     G_APPLICATION_NON_UNIQUE);
  g_object_set_data (G_OBJECT (application), "capture-width", GINT_TO_POINTER ((int) parsed_width));
  g_object_set_data (G_OBJECT (application), "capture-height", GINT_TO_POINTER ((int) parsed_height));
  g_object_set_data (G_OBJECT (application), "capture-backwards", GINT_TO_POINTER (backwards));
  g_signal_connect (application, "activate", G_CALLBACK (activate_cb), &capture);
  return g_application_run (G_APPLICATION (application), 1, argv);
}
