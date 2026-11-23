/* The C interface of the chat core for the phone apps. Text is UTF-8. A string the library returns
   is freed with cf_chat_free. */
#ifndef CONSTRUCT_CHAT_H
#define CONSTRUCT_CHAT_H

#include <stdint.h>

/* Builds the request body of the fixed test conversation over the tool file `tools_file` (the text
   of the file). Returns 0 and sets *out to the body, or 1 and sets *out to the plain reason. */
int32_t cf_chat_fixed_request_body(const char *tools_file, char **out);

/* Tests a model setting and returns the JSON line the desktop's Settings page gets:
   {"result":"works",...}, {"result":"key refused"}, {"error":"..."} and so on. The key is an
   argument only: it is in no log and in no returned text. */
char *cf_chat_test_model(const char *base_url, const char *model, const char *api_key);

typedef struct cf_chat_stop cf_chat_stop;
cf_chat_stop *cf_chat_stop_new(void);
void cf_chat_stop_answer(const cf_chat_stop *stop);
void cf_chat_stop_free(cf_chat_stop *stop);

/* Runs one answer on the calling thread. The callback borrows text for the duration of its call.
   Kind 0 is a chat-send event and kind 1 is a record to commit before returning. Return 1 on
   success, or 0 when the event cannot be delivered or the record cannot be written.
   Another thread may request a stop. Free the token only after the send and stop calls return. */
typedef int32_t (*cf_chat_callback)(void *context, int32_t kind, const char *text);
void cf_chat_send(const char *input, const cf_chat_stop *stop, cf_chat_callback callback, void *context);

void cf_chat_free(char *text);

#endif
