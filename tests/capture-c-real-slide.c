#include "config.h"
#include "pp-presentation.h"
#include "pp-stage.h"

#include <gtk/gtk.h>
#include <gst/gst.h>

typedef struct
{
  GtkApplication *application;
  GtkWidget *stage;
  const char *output;
} Capture;

static gboolean
capture_cb (gpointer user_data)
{
  Capture *capture = user_data;
  GtkNative *native = gtk_widget_get_native (capture->stage);
  g_autoptr (GdkPaintable) paintable = NULL;
  g_autoptr (GtkSnapshot) snapshot = NULL;
  g_autoptr (GskRenderNode) node = NULL;
  g_autoptr (GdkTexture) texture = NULL;
  GskRenderer *renderer;

  if (native == NULL)
    g_error ("C real slide capture has no GtkNative");
  renderer = gtk_native_get_renderer (native);
  paintable = gtk_widget_paintable_new (capture->stage);
  snapshot = gtk_snapshot_new ();
  gdk_paintable_snapshot (paintable,
                          GDK_SNAPSHOT (snapshot),
                          gtk_widget_get_width (capture->stage),
                          gtk_widget_get_height (capture->stage));
  node = gtk_snapshot_free_to_node (g_steal_pointer (&snapshot));
  if (node == NULL)
    g_error ("C real slide capture snapshot was empty");
  texture = gsk_renderer_render_texture (renderer, node, NULL);
  if (!gdk_texture_save_to_png (texture, capture->output))
    g_error ("Unable to save C real slide capture");
  g_print ("CURL REAL CAPTURE C output=%s size=%dx%d\n",
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
  const char *presentation_path = g_object_get_data (G_OBJECT (application), "presentation");
  guint slide = GPOINTER_TO_UINT (g_object_get_data (G_OBJECT (application), "slide"));
  g_autoptr (GFile) file = g_file_new_for_commandline_arg (presentation_path);
  g_autoptr (GError) error = NULL;
  g_autoptr (PpPresentation) presentation = pp_presentation_load (file,
                                                                    FALSE,
                                                                    NULL,
                                                                    &error);
  GtkWindow *window;

  if (presentation == NULL)
    g_error ("Unable to load presentation: %s", error->message);
  if (slide >= pp_presentation_get_n_slides (presentation))
    g_error ("Slide %u is outside presentation with %u slides",
             slide,
             pp_presentation_get_n_slides (presentation));

  capture->application = application;
  capture->stage = pp_stage_new ();
  pp_stage_set_presentation (PP_STAGE (capture->stage), presentation, slide);
  window = GTK_WINDOW (gtk_application_window_new (application));
  gtk_window_set_decorated (window, FALSE);
  gtk_window_set_default_size (window, 1280, 720);
  gtk_window_set_child (window, capture->stage);
  gtk_window_present (window);
  g_timeout_add (2000, capture_cb, capture);
}

int
main (int   argc,
      char *argv[])
{
  g_autoptr (GtkApplication) application = NULL;
  Capture capture = { 0 };
  guint64 slide;

  if (argc != 4 ||
      !g_ascii_string_to_unsigned (argv[2], 10, 0, G_MAXUINT, &slide, NULL))
    {
      g_printerr ("usage: %s PRESENTATION.pin SLIDE OUTPUT.png\n", argv[0]);
      return 2;
    }
  capture.output = argv[3];
  gst_init (&argc, &argv);
  application = gtk_application_new ("com.nedrichards.Pinpoint.RealCapture",
                                     G_APPLICATION_NON_UNIQUE);
  g_object_set_data (G_OBJECT (application), "presentation", argv[1]);
  g_object_set_data (G_OBJECT (application), "slide", GUINT_TO_POINTER ((guint) slide));
  g_signal_connect (application, "activate", G_CALLBACK (activate_cb), &capture);
  return g_application_run (G_APPLICATION (application), 1, argv);
}
