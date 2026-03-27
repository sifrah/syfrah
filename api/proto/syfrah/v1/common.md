# Common API Reference

Package: `syfrah.v1`

## Messages

### Empty}

| Field | Type | Number |
|-------|------|--------|
### Error

| Field | Type | Number |
|-------|------|--------|
| `code` | `string` | 1 |
| `message` | `string` | 2 |

### PaginationRequest

| Field | Type | Number |
|-------|------|--------|
| `page_size` | `uint32` | 1 |
| `page_token` | `string` | 2 |

### PaginationResponse

| Field | Type | Number |
|-------|------|--------|
| `next_page_token` | `string` | 1 |

---

Source: `api/proto/syfrah/v1/common.proto`
