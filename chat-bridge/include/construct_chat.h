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

void cf_chat_free(char *text);

#endif
