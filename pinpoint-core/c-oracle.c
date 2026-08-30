#include "pp-presentation.h"
#include "pp-source.h"
#include "pp-transition.h"

#include <json-glib/json-glib.h>
#include <stdlib.h>

/* Keep the oracle link small: pp-transition.c needs only this pp-render seam. */
GFile *
pp_render_resolve_asset (const PpPresentation *presentation,
                         const char           *asset)
{
  GFile *presentation_file = pp_presentation_get_file (presentation);
  g_autoptr (GFile) parent = NULL;

  if (g_path_is_absolute (asset))
    return g_file_new_for_path (asset);
  if (presentation_file == NULL)
    return g_file_new_for_path (asset);
  parent = g_file_get_parent (presentation_file);
  return parent != NULL ? g_file_resolve_relative_path (parent, asset)
                        : g_file_new_for_path (asset);
}

static const char *
gravity_name (PpGravity value)
{
  static const char *const names[] = {
    "Center", "Top", "Bottom", "Left", "Right",
    "TopLeft", "TopRight", "BottomLeft", "BottomRight",
  };
  return names[value];
}

static const char *
text_align_name (PpTextAlign value)
{
  static const char *const names[] = { "Left", "Center", "Right" };
  return names[value];
}

static const char *
background_type_name (PpBackgroundType value)
{
  static const char *const names[] = {
    "None", "Color", "Image", "Video", "Camera", "Svg",
  };
  return names[value];
}

static const char *
background_scale_name (PpBackgroundScale value)
{
  static const char *const names[] = { "Unscaled", "Fit", "Fill", "Stretch" };
  return names[value];
}

static const char *
direction_name (PpTransitionDirection value)
{
  static const char *const names[] = { "Left", "Right", "Up", "Down" };
  return names[value];
}

static const char *
layer_name (PpTransitionLayer value)
{
  static const char *const names[] = { "Default", "All", "Background", "Text" };
  return names[value];
}

static const char *
mode_name (PpTransitionMode value)
{
  static const char *const names[] = { "Both", "In", "Out" };
  return names[value];
}

static void
add_string (JsonBuilder *builder,
            const char  *name,
            const char  *value)
{
  json_builder_set_member_name (builder, name);
  if (value != NULL)
    json_builder_add_string_value (builder, value);
  else
    json_builder_add_null_value (builder);
}

static void
add_slide (JsonBuilder   *builder,
           const PpSlide *slide)
{
  json_builder_begin_object (builder);
  add_string (builder, "stage_color", slide->stage_color);
  add_string (builder, "background", slide->background);
  add_string (builder, "background_type", background_type_name (slide->background_type));
  add_string (builder, "background_scale", background_scale_name (slide->background_scale));
  add_string (builder, "background_position", gravity_name (slide->background_position));
  add_string (builder, "text", slide->text);
  add_string (builder, "text_position", gravity_name (slide->text_position));
  add_string (builder, "font", slide->font);
  add_string (builder, "notes_font", slide->notes_font);
  add_string (builder, "notes_font_size", slide->notes_font_size);
  add_string (builder, "text_align", text_align_name (slide->text_align));
  add_string (builder, "text_color", slide->text_color);
  json_builder_set_member_name (builder, "use_markup");
  json_builder_add_boolean_value (builder, slide->use_markup);
  json_builder_set_member_name (builder, "duration");
  json_builder_add_double_value (builder, slide->duration);
  json_builder_set_member_name (builder, "new_duration");
  json_builder_add_double_value (builder, slide->new_duration);
  add_string (builder, "speaker_notes", slide->speaker_notes);
  add_string (builder, "visual_description", slide->visual_description);
  add_string (builder, "shading_color", slide->shading_color);
  json_builder_set_member_name (builder, "shading_opacity");
  json_builder_add_double_value (builder, slide->shading_opacity);
  add_string (builder, "transition", slide->transition);
  add_string (builder, "transition_direction", direction_name (slide->transition_direction));
  add_string (builder, "transition_layer", layer_name (slide->transition_layer));
  add_string (builder, "transition_mode", mode_name (slide->transition_mode));
  json_builder_set_member_name (builder, "transition_duration_ms");
  json_builder_add_int_value (builder, slide->transition_duration_ms);
  add_string (builder, "transition_easing", slide->transition_easing);
  add_string (builder, "command", slide->command);
  json_builder_set_member_name (builder, "camera_framerate");
  json_builder_add_int_value (builder, slide->camera_framerate);
  json_builder_set_member_name (builder, "camera_resolution");
  json_builder_begin_object (builder);
  json_builder_set_member_name (builder, "width");
  json_builder_add_int_value (builder, slide->camera_resolution.width);
  json_builder_set_member_name (builder, "height");
  json_builder_add_int_value (builder, slide->camera_resolution.height);
  json_builder_end_object (builder);
  json_builder_end_object (builder);
}

static gboolean
emit_presentation (const char *filename,
                   gboolean    ignore_comments,
                   GError    **error)
{
  g_autoptr (GFile) file = g_file_new_for_path (filename);
  g_autoptr (PpPresentation) presentation = NULL;
  g_autoptr (JsonBuilder) builder = json_builder_new ();
  g_autoptr (JsonGenerator) generator = json_generator_new ();
  g_autoptr (JsonNode) root = NULL;

  presentation = pp_presentation_load (file, ignore_comments, NULL, error);
  if (presentation == NULL)
    return FALSE;
  json_builder_begin_object (builder);
  json_builder_set_member_name (builder, "defaults");
  add_slide (builder, pp_presentation_get_defaults (presentation));
  json_builder_set_member_name (builder, "slides");
  json_builder_begin_array (builder);
  for (guint i = 0; i < pp_presentation_get_n_slides (presentation); i++)
    add_slide (builder, pp_presentation_get_slide (presentation, i));
  json_builder_end_array (builder);
  json_builder_end_object (builder);
  root = json_builder_get_root (builder);
  json_generator_set_root (generator, root);
  {
    g_autofree char *json = json_generator_to_data (generator, NULL);
    g_print ("%s\n", json);
  }
  return TRUE;
}

static gboolean
emit_source (const char *filename,
             GError    **error)
{
  g_autofree char *source = NULL;
  g_autoptr (PpSourceAnalysis) analysis = NULL;
  g_autoptr (JsonBuilder) builder = json_builder_new ();
  g_autoptr (JsonGenerator) generator = json_generator_new ();
  g_autoptr (JsonNode) root = NULL;

  if (!g_file_get_contents (filename, &source, NULL, error))
    return FALSE;
  analysis = pp_source_analyze (source, NULL);
  json_builder_begin_object (builder);
  json_builder_set_member_name (builder, "slides");
  json_builder_begin_array (builder);
  for (guint i = 0; i < pp_source_analysis_get_n_slides (analysis); i++)
    {
      const PpSourceSlide *slide = pp_source_analysis_get_slide (analysis, i);
      json_builder_begin_object (builder);
#define ADD_OFFSET(member) \
      json_builder_set_member_name (builder, #member); \
      json_builder_add_int_value (builder, slide->member)
      ADD_OFFSET (start);
      ADD_OFFSET (separator_end);
      ADD_OFFSET (end);
#undef ADD_OFFSET
      add_string (builder, "title", slide->title);
      json_builder_end_object (builder);
    }
  json_builder_end_array (builder);
  json_builder_set_member_name (builder, "diagnostics");
  json_builder_begin_array (builder);
  for (guint i = 0; i < pp_source_analysis_get_n_diagnostics (analysis); i++)
    {
      const PpSourceDiagnostic *diagnostic = pp_source_analysis_get_diagnostic (analysis, i);
      json_builder_begin_object (builder);
      json_builder_set_member_name (builder, "start");
      json_builder_add_int_value (builder, diagnostic->start);
      json_builder_set_member_name (builder, "end");
      json_builder_add_int_value (builder, diagnostic->end);
      add_string (builder, "severity", diagnostic->severity == PP_SOURCE_DIAGNOSTIC_ERROR ? "Error" : "Warning");
      add_string (builder, "message", diagnostic->message);
      json_builder_end_object (builder);
    }
  json_builder_end_array (builder);
  json_builder_end_object (builder);
  root = json_builder_get_root (builder);
  json_generator_set_root (generator, root);
  {
    g_autofree char *json = json_generator_to_data (generator, NULL);
    g_print ("%s\n", json);
  }
  return TRUE;
}

static void
add_transition_layer (JsonBuilder                  *builder,
                      const PpTransitionLayerState *layer)
{
  json_builder_begin_object (builder);
#define ADD_FLOAT(member) \
  json_builder_set_member_name (builder, #member); \
  json_builder_add_double_value (builder, layer->member)
  ADD_FLOAT (x);
  ADD_FLOAT (y);
  ADD_FLOAT (scale_x);
  ADD_FLOAT (scale_y);
  ADD_FLOAT (angle);
  ADD_FLOAT (angle_x);
  ADD_FLOAT (angle_y);
  ADD_FLOAT (opacity);
#undef ADD_FLOAT
  json_builder_end_object (builder);
}

static void
add_transition_state (JsonBuilder             *builder,
                      const PpTransitionState *state)
{
  json_builder_begin_object (builder);
#define ADD_LAYER(member) \
  json_builder_set_member_name (builder, #member); \
  add_transition_layer (builder, &state->member)
  ADD_LAYER (actor);
  ADD_LAYER (background);
  ADD_LAYER (midground);
  ADD_LAYER (foreground);
#undef ADD_LAYER
  json_builder_end_object (builder);
}

static gboolean
emit_transition (const char *filename,
                 GError    **error)
{
  static const gboolean incoming[] = { TRUE, TRUE, FALSE, TRUE, FALSE };
  static const gboolean backwards[] = { FALSE, FALSE, FALSE, TRUE, TRUE };
  static const double progress[] = { 0.0, 0.5, 1.0, 1.0, 1.0 };
  g_autoptr (GFile) file = g_file_new_for_path (filename);
  g_autoptr (PpPresentation) presentation = NULL;
  g_autoptr (PpLegacyTransition) transition = NULL;
  g_autoptr (JsonBuilder) builder = json_builder_new ();
  g_autoptr (JsonGenerator) generator = json_generator_new ();
  g_autoptr (JsonNode) root = NULL;
  const PpSlide *slide;

  presentation = pp_presentation_load (file, FALSE, NULL, error);
  if (presentation == NULL)
    return FALSE;
  slide = pp_presentation_get_slide (presentation, 0);
  transition = pp_legacy_transition_load (presentation, slide->transition, error);
  if (transition == NULL)
    return FALSE;
  json_builder_begin_object (builder);
  json_builder_set_member_name (builder, "durations");
  json_builder_begin_array (builder);
  json_builder_add_int_value (builder, pp_legacy_transition_get_duration (transition, TRUE, FALSE));
  json_builder_add_int_value (builder, pp_legacy_transition_get_duration (transition, FALSE, FALSE));
  json_builder_add_int_value (builder, pp_legacy_transition_get_duration (transition, FALSE, TRUE));
  json_builder_end_array (builder);
  json_builder_set_member_name (builder, "samples");
  json_builder_begin_array (builder);
  for (guint i = 0; i < G_N_ELEMENTS (incoming); i++)
    {
      PpTransitionState state;
      pp_legacy_transition_calculate (transition, incoming[i], backwards[i], progress[i], &state);
      add_transition_state (builder, &state);
    }
  json_builder_end_array (builder);
  json_builder_end_object (builder);
  root = json_builder_get_root (builder);
  json_generator_set_root (generator, root);
  {
    g_autofree char *json = json_generator_to_data (generator, NULL);
    g_print ("%s\n", json);
  }
  return TRUE;
}

int
main (int   argc,
      char *argv[])
{
  g_autoptr (GError) error = NULL;
  gboolean success = FALSE;

  if (argc != 3)
    {
      g_printerr ("usage: pinpoint-c-oracle MODE FILE\n");
      return EXIT_FAILURE;
    }
  if (g_str_equal (argv[1], "presentation"))
    success = emit_presentation (argv[2], FALSE, &error);
  else if (g_str_equal (argv[1], "presentation-ignore-comments"))
    success = emit_presentation (argv[2], TRUE, &error);
  else if (g_str_equal (argv[1], "source"))
    success = emit_source (argv[2], &error);
  else if (g_str_equal (argv[1], "transition"))
    success = emit_transition (argv[2], &error);
  else
    g_set_error (&error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                 "unknown oracle mode: %s", argv[1]);
  if (!success)
    {
      g_printerr ("%s\n", error != NULL ? error->message : "oracle failed");
      return EXIT_FAILURE;
    }
  return EXIT_SUCCESS;
}
