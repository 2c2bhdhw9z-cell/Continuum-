/*
 * Continuum - the flat C face of rcheevos' rc_client, for the Rust engine.
 *
 * WHY A SHIM AT ALL: rc_client hands its callers structs (rc_client_user_t, rc_client_event_t,
 * rc_client_achievement_t, rc_api_server_response_t) whose layouts include time_t, float, fixed
 * char arrays and pointers. Mirroring each of those in Rust is exactly the hand-declared-struct
 * mistake this project has paid for before (see the libretro header notes in SESSION_HANDOFF.md).
 * So every field is read HERE, by the compiler that read the real header, and Rust only ever sees
 * opaque pointers, integers and NUL-terminated strings.
 *
 * Every hook is called synchronously from inside an rc_client call that Rust made, on the same
 * thread, while Rust holds the engine lock. A hook must never call back into the engine.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "rc_client.h"
#include "rc_consoles.h"
#include "rc_error.h"

/* Copies a C string into a fixed buffer, always terminated, null treated as empty. */
static void copy_out(char* dst, size_t size, const char* src) {
  if (!dst || !size)
    return;
  strncpy(dst, src ? src : "", size);
  dst[size - 1] = '\0';
}

/* ------------------------------------------------------------------ hooks into Rust */

typedef uint32_t (*continuum_rc_read_hook)(void* ctx, uint32_t address, uint8_t* buffer,
                                           uint32_t num_bytes);
/* `callback` and `callback_data` are opaque to Rust and come back in continuum_rc_complete. */
typedef void (*continuum_rc_server_hook)(void* ctx, const char* url, const char* post_data,
                                         const char* content_type, void* callback,
                                         void* callback_data);
/* kind: see CONTINUUM_RC_ASYNC_* below. result is an RC_ error code, 0 = RC_OK. */
typedef void (*continuum_rc_async_hook)(void* ctx, int kind, int result, const char* message);
typedef void (*continuum_rc_event_hook)(void* ctx, uint32_t type, uint32_t achievement_id,
                                        const char* title, const char* description,
                                        uint32_t points, const char* badge_url,
                                        const char* detail);
typedef void (*continuum_rc_log_hook)(void* ctx, const char* message);
typedef void (*continuum_rc_achievement_visitor)(void* visitor_ctx, uint32_t id,
                                                 const char* title, const char* description,
                                                 uint32_t points, int unlocked,
                                                 uint32_t bucket, const char* badge_url,
                                                 const char* badge_locked_url,
                                                 const char* measured_progress,
                                                 int64_t unlock_time);

#define CONTINUUM_RC_ASYNC_LOGIN 1
#define CONTINUUM_RC_ASYNC_LOAD_GAME 2

typedef struct continuum_rc_ctx {
  void* rust;
  continuum_rc_read_hook read;
  continuum_rc_server_hook server;
  continuum_rc_async_hook async;
  continuum_rc_event_hook event;
  continuum_rc_log_hook log;
} continuum_rc_ctx;

static continuum_rc_ctx* ctx_of(const rc_client_t* client) {
  return (continuum_rc_ctx*)rc_client_get_userdata(client);
}

/* ------------------------------------------------------------------ rc_client callbacks */

static uint32_t RC_CCONV shim_read_memory(uint32_t address, uint8_t* buffer, uint32_t num_bytes,
                                          rc_client_t* client) {
  continuum_rc_ctx* ctx = ctx_of(client);
  if (!ctx || !ctx->read)
    return 0;
  return ctx->read(ctx->rust, address, buffer, num_bytes);
}

static void RC_CCONV shim_server_call(const rc_api_request_t* request,
                                      rc_client_server_callback_t callback, void* callback_data,
                                      rc_client_t* client) {
  continuum_rc_ctx* ctx = ctx_of(client);
  if (!ctx || !ctx->server) {
    rc_api_server_response_t response;
    memset(&response, 0, sizeof(response));
    response.http_status_code = RC_API_SERVER_RESPONSE_CLIENT_ERROR;
    callback(&response, callback_data);
    return;
  }
  ctx->server(ctx->rust, request->url, request->post_data, request->content_type,
              (void*)callback, callback_data);
}

static void RC_CCONV shim_log(const char* message, const rc_client_t* client) {
  continuum_rc_ctx* ctx = ctx_of(client);
  if (ctx && ctx->log)
    ctx->log(ctx->rust, message);
}

static void RC_CCONV shim_event(const rc_client_event_t* event, rc_client_t* client) {
  continuum_rc_ctx* ctx = ctx_of(client);
  char url[512];
  if (!ctx || !ctx->event)
    return;
  url[0] = '\0';
  if (event->achievement) {
    const rc_client_achievement_t* a = event->achievement;
    if (a->badge_url)
      copy_out(url, sizeof(url), a->badge_url);
    else
      rc_client_achievement_get_image_url(a, RC_CLIENT_ACHIEVEMENT_STATE_UNLOCKED, url,
                                          sizeof(url));
    ctx->event(ctx->rust, event->type, a->id, a->title, a->description, a->points, url,
               a->measured_progress);
    return;
  }
  if (event->server_error) {
    ctx->event(ctx->rust, event->type, 0, event->server_error->api,
               event->server_error->error_message, 0, "", "");
    return;
  }
  if (event->leaderboard) {
    ctx->event(ctx->rust, event->type, event->leaderboard->id, event->leaderboard->title,
               event->leaderboard->description, 0, "",
               event->leaderboard->tracker_value ? event->leaderboard->tracker_value : "");
    return;
  }
  if (event->subset) {
    ctx->event(ctx->rust, event->type, event->subset->id, event->subset->title, "", 0,
               event->subset->badge_url ? event->subset->badge_url : "", "");
    return;
  }
  ctx->event(ctx->rust, event->type, 0, "", "", 0, "", "");
}

static void RC_CCONV shim_login_done(int result, const char* error_message, rc_client_t* client,
                                     void* userdata) {
  continuum_rc_ctx* ctx = ctx_of(client);
  (void)userdata;
  if (ctx && ctx->async)
    ctx->async(ctx->rust, CONTINUUM_RC_ASYNC_LOGIN, result, error_message ? error_message : "");
}

static void RC_CCONV shim_load_done(int result, const char* error_message, rc_client_t* client,
                                    void* userdata) {
  continuum_rc_ctx* ctx = ctx_of(client);
  (void)userdata;
  if (ctx && ctx->async)
    ctx->async(ctx->rust, CONTINUUM_RC_ASYNC_LOAD_GAME, result,
               error_message ? error_message : "");
}

/* ------------------------------------------------------------------ the API Rust calls */

rc_client_t* continuum_rc_create(void* rust, continuum_rc_read_hook read,
                                 continuum_rc_server_hook server, continuum_rc_async_hook async,
                                 continuum_rc_event_hook event, continuum_rc_log_hook log) {
  continuum_rc_ctx* ctx = (continuum_rc_ctx*)calloc(1, sizeof(continuum_rc_ctx));
  rc_client_t* client;
  if (!ctx)
    return NULL;
  ctx->rust = rust;
  ctx->read = read;
  ctx->server = server;
  ctx->async = async;
  ctx->event = event;
  ctx->log = log;
  client = rc_client_create(shim_read_memory, shim_server_call);
  if (!client) {
    free(ctx);
    return NULL;
  }
  rc_client_set_userdata(client, ctx);
  rc_client_set_event_handler(client, shim_event);
  rc_client_enable_logging(client, RC_CLIENT_LOG_LEVEL_INFO, shim_log);
  /* Softcore by default, which is the owner's rule: hardcore forbids save states, rewind,
   * cheats and slow motion, and every one of those is a feature of this app. */
  rc_client_set_hardcore_enabled(client, 0);
  return client;
}

void continuum_rc_destroy(rc_client_t* client) {
  continuum_rc_ctx* ctx;
  if (!client)
    return;
  ctx = ctx_of(client);
  rc_client_destroy(client);
  free(ctx);
}

/* Hands an HTTP response back to the rc_client callback that asked for it. */
void continuum_rc_complete(void* callback, void* callback_data, const char* body,
                           size_t body_length, int http_status) {
  rc_api_server_response_t response;
  rc_client_server_callback_t fn = (rc_client_server_callback_t)callback;
  if (!fn)
    return;
  memset(&response, 0, sizeof(response));
  response.body = body;
  response.body_length = body_length;
  response.http_status_code = http_status;
  fn(&response, callback_data);
}

void continuum_rc_login_password(rc_client_t* client, const char* user, const char* password) {
  rc_client_begin_login_with_password(client, user, password, shim_login_done, NULL);
}

void continuum_rc_login_token(rc_client_t* client, const char* user, const char* token) {
  rc_client_begin_login_with_token(client, user, token, shim_login_done, NULL);
}

void continuum_rc_logout(rc_client_t* client) { rc_client_logout(client); }

/* Copies the logged-in user's facts out. Returns 0 when nobody is logged in. */
int continuum_rc_user(rc_client_t* client, char* username, size_t username_size,
                      char* display_name, size_t display_size, char* token, size_t token_size,
                      uint32_t* score, uint32_t* score_softcore) {
  const rc_client_user_t* user = rc_client_get_user_info(client);
  if (!user)
    return 0;
  copy_out(username, username_size, user->username);
  copy_out(display_name, display_size, user->display_name);
  copy_out(token, token_size, user->token);
  if (score)
    *score = user->score;
  if (score_softcore)
    *score_softcore = user->score_softcore;
  return 1;
}

void continuum_rc_load_game(rc_client_t* client, uint32_t console_id, const char* path,
                            const uint8_t* data, size_t size) {
  rc_client_begin_identify_and_load_game(client, console_id, path, data, size, shim_load_done,
                                         NULL);
}

void continuum_rc_unload_game(rc_client_t* client) { rc_client_unload_game(client); }

/* Copies the loaded game's facts out. Returns 0 when no game is loaded. */
int continuum_rc_game(rc_client_t* client, uint32_t* id, char* title, size_t title_size,
                      char* hash, size_t hash_size, char* badge_url, size_t badge_size,
                      uint32_t* core_count, uint32_t* unlocked_count, uint32_t* points_core,
                      uint32_t* points_unlocked) {
  const rc_client_game_t* game = rc_client_get_game_info(client);
  rc_client_user_game_summary_t summary;
  if (!game || !rc_client_is_game_loaded(client))
    return 0;
  if (id)
    *id = game->id;
  copy_out(title, title_size, game->title);
  copy_out(hash, hash_size, game->hash);
  if (badge_url && badge_size) {
    if (game->badge_url)
      copy_out(badge_url, badge_size, game->badge_url);
    else
      rc_client_game_get_image_url(game, badge_url, badge_size);
  }
  memset(&summary, 0, sizeof(summary));
  rc_client_get_user_game_summary(client, &summary);
  if (core_count)
    *core_count = summary.num_core_achievements;
  if (unlocked_count)
    *unlocked_count = summary.num_unlocked_achievements;
  if (points_core)
    *points_core = summary.points_core;
  if (points_unlocked)
    *points_unlocked = summary.points_unlocked;
  return 1;
}

/* Calls `visit` once per achievement, grouped by progress the way rc_client's own UI lists are. */
void continuum_rc_list_achievements(rc_client_t* client, void* visitor_ctx,
                                    continuum_rc_achievement_visitor visit) {
  rc_client_achievement_list_t* list;
  uint32_t b, i;
  char unlocked_url[512], locked_url[512];
  if (!visit)
    return;
  list = rc_client_create_achievement_list(client, RC_CLIENT_ACHIEVEMENT_CATEGORY_CORE,
                                           RC_CLIENT_ACHIEVEMENT_LIST_GROUPING_PROGRESS);
  if (!list)
    return;
  for (b = 0; b < list->num_buckets; ++b) {
    const rc_client_achievement_bucket_t* bucket = &list->buckets[b];
    for (i = 0; i < bucket->num_achievements; ++i) {
      const rc_client_achievement_t* a = bucket->achievements[i];
      unlocked_url[0] = locked_url[0] = '\0';
      if (a->badge_url)
        copy_out(unlocked_url, sizeof(unlocked_url), a->badge_url);
      else
        rc_client_achievement_get_image_url(a, RC_CLIENT_ACHIEVEMENT_STATE_UNLOCKED,
                                            unlocked_url, sizeof(unlocked_url));
      if (a->badge_locked_url)
        copy_out(locked_url, sizeof(locked_url), a->badge_locked_url);
      else
        rc_client_achievement_get_image_url(a, RC_CLIENT_ACHIEVEMENT_STATE_ACTIVE, locked_url,
                                            sizeof(locked_url));
      visit(visitor_ctx, a->id, a->title ? a->title : "", a->description ? a->description : "",
            a->points, a->unlocked != 0, bucket->bucket_type, unlocked_url, locked_url,
            a->measured_progress, (int64_t)a->unlock_time);
    }
  }
  rc_client_destroy_achievement_list(list);
}

void continuum_rc_do_frame(rc_client_t* client) { rc_client_do_frame(client); }
void continuum_rc_idle(rc_client_t* client) { rc_client_idle(client); }
void continuum_rc_reset(rc_client_t* client) { rc_client_reset(client); }
int continuum_rc_is_game_loaded(rc_client_t* client) { return rc_client_is_game_loaded(client); }
int continuum_rc_get_hardcore(rc_client_t* client) {
  return rc_client_get_hardcore_enabled(client);
}
void continuum_rc_set_hardcore(rc_client_t* client, int enabled) {
  rc_client_set_hardcore_enabled(client, enabled);
}

/* The console's memory map, one region at a time. Returns 0 past the end. */
int continuum_rc_console_region(uint32_t console_id, uint32_t index, uint32_t* start,
                                uint32_t* end, uint8_t* type) {
  const rc_memory_regions_t* regions = rc_console_memory_regions(console_id);
  if (!regions || index >= regions->num_regions)
    return 0;
  *start = regions->region[index].start_address;
  *end = regions->region[index].end_address;
  *type = regions->region[index].type;
  return 1;
}

/* rc_error_str, for readable status lines. */
const char* continuum_rc_error_str(int result) { return rc_error_str(result); }

/* "rcheevos/12.5" and similar, for the User-Agent the RetroAchievements server asks for. */
size_t continuum_rc_user_agent_clause(rc_client_t* client, char* buffer, size_t size) {
  return rc_client_get_user_agent_clause(client, buffer, size);
}
