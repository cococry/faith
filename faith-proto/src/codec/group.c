#include "codec/group.h"
#include "codec/helpers.h"

#include <string.h>

faith_status_code_t faith_group_encode(uint8_t *out, size_t cap, size_t *size,
                                       const faith_group_packet_t *p) {
  if (!out || !size || !p || p->format != FAITH_GROUP_PLAINTEXT_V1 ||
      p->kind < FAITH_GROUP_CREATE || p->kind > FAITH_GROUP_MESSAGE ||
      p->n_members == 0 || p->n_members > FAITH_GROUP_MAX_MEMBERS ||
      p->text_size > FAITH_GROUP_MAX_TEXT || (p->text_size && !p->text))
    return FAITH_ERR_INVALID;

  size_t n = FAITH_GROUP_HEADER_SIZE + 16 * p->n_members + p->text_size;

  if (cap < n)
    return FAITH_ERR_OVERFLOW;

  size_t  offset = 0;
  uint8_t kind = p->kind;

  FAITH_APPEND_RETURN(out, cap, offset, &p->format, sizeof(p->format));
  FAITH_APPEND_RETURN(out, cap, offset, &kind, sizeof(kind));
  FAITH_ENCODE_U16_BE_RETURN(out, cap, offset, p->n_members);
  FAITH_ENCODE_U64_BE_RETURN(out, cap, offset, p->revision);
  FAITH_APPEND_RETURN(out, cap, offset, p->conversation_id,
                      sizeof(p->conversation_id));
  FAITH_APPEND_RETURN(out, cap, offset, p->message_id, sizeof(p->message_id));
  FAITH_ENCODE_U32_BE_RETURN(out, cap, offset, p->text_size);
  FAITH_APPEND_RETURN(out, cap, offset, p->members,
                      sizeof(p->members[0]) * p->n_members);
  FAITH_APPEND_RETURN(out, cap, offset, p->text, p->text_size);

  *size = offset;
  faith_group_packet_t checked;

  return faith_group_decode(out, n, &checked);
}

faith_status_code_t faith_group_decode(const uint8_t *data, size_t size,
                                       faith_group_packet_t *p) {
  if (!data || !p || size < FAITH_GROUP_HEADER_SIZE)
    return FAITH_ERR_INVALID;

  memset(p, 0, sizeof(*p));
  size_t  offset = 0;
  uint8_t kind;

  FAITH_DECODE_RETURN(data, size, offset, &p->format, sizeof(p->format));
  FAITH_DECODE_RETURN(data, size, offset, &kind, sizeof(kind));
  FAITH_DECODE_U16_BE_RETURN(data, size, offset, p->n_members);
  FAITH_DECODE_U64_BE_RETURN(data, size, offset, p->revision);
  FAITH_DECODE_RETURN(data, size, offset, p->conversation_id,
                      sizeof(p->conversation_id));
  FAITH_DECODE_RETURN(data, size, offset, p->message_id, sizeof(p->message_id));
  FAITH_DECODE_U32_BE_RETURN(data, size, offset, p->text_size);

  p->kind = kind;

  if (p->format != FAITH_GROUP_PLAINTEXT_V1 || p->kind < FAITH_GROUP_CREATE ||
      p->kind > FAITH_GROUP_MESSAGE || !p->n_members ||
      p->n_members > FAITH_GROUP_MAX_MEMBERS ||
      p->text_size > FAITH_GROUP_MAX_TEXT ||
      size != FAITH_GROUP_HEADER_SIZE + 16u * p->n_members + p->text_size ||
      p->revision > INT64_MAX ||
      (p->kind != FAITH_GROUP_MESSAGE && p->text_size) ||
      (p->kind == FAITH_GROUP_CREATE && p->revision != 0))
    return FAITH_ERR_BAD_ENVELOPE;

  FAITH_DECODE_RETURN(data, size, offset, p->members,
                      sizeof(p->members[0]) * p->n_members);

  p->text = data + offset;

  /* reject duplicate members */
  for (size_t i = 0; i < p->n_members; i++)
    for (size_t j = 0; j < i; j++)
      if (!memcmp(&p->members[i], &p->members[j], 16))
        return FAITH_ERR_INVALID;

  return FAITH_OK;
}
