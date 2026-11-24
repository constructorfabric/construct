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

/* TLS 1.3 phone channel. A channel opened with a null pin accepts any certificate: it only
   carries a typed pairing (cf_phone_pair_code), which authenticates the computer itself, and
   cf_phone_exchange refuses it. A channel is used by one caller at a time. Free it once with
   cf_phone_close. */
typedef void cf_phone_channel;
uint8_t cf_phone_is_numeric_address(const char *host);
/* Returns 0 with a channel in *out, 1 if unavailable, 2 on a certificate pin mismatch,
   or 4 when a channel with a null pin finds that the TLS service at the address does not offer
   the typed pairing protocol: it rejected it during negotiation or completed the handshake
   without it. That is an older Construct, but it may be any other TLS service. Nothing has been
   sent and the computer is not authenticated, so 4 means only "this address does not speak the
   typed pairing protocol"; the caller decides what to tell the person. */
int32_t cf_phone_open(const char *host, uint16_t port, const char *pin, cf_phone_channel **out);
char *cf_phone_fingerprint(const cf_phone_channel *channel);
/* Sends one line and reads one line on a channel opened with a pin. Returns 0 with a line in
   *out (free with cf_chat_free), 1 for a failed exchange or a channel without a pin, or 2 for an
   authenticated TLS close without a line. */
int32_t cf_phone_exchange(cf_phone_channel *channel, const char *line, char **out);
/* Pairs over the channel and returns 0 with an authenticated line in *out (free with cf_chat_free),
   1 for a failed exchange or 3 if the code was refused. */
int32_t cf_phone_pair_code(cf_phone_channel *channel, const char *code, uint64_t version,
                           const char *name, char **out);
void cf_phone_close(cf_phone_channel *channel);
/* HMAC over the pairing role and certificate fingerprint, returned as hex. A nonzero agent
   flag selects the agent's proof. The key and fingerprint are hex. Free with cf_chat_free. */
char *cf_phone_proof(const char *key, const char *fingerprint, uint8_t agent);

#endif
