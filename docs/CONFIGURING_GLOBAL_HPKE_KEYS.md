# Global HPKE Keypairs
Janus has support for HPKE key advertisement on `/hpke_configs` that are not
tied to tasks. This is mainly to support the Taskprov extension, but we are
also [considering having this replace per-task keys entirely][1].

If a global key is configured, requests to `/hpke_configs` that don't provide
the task ID will provide the list of active global HPKE keys.

This document describes the operational overhead of managing these keys.

[1]: https://github.com/divviup/janus/issues/1641

## Lifecycle

A global keypair has three states:
- `pending`: The key is in the database, but is not being advertised to clients.
- `active`: The key is being advertised to clients, and clients should use it to encrypt
  reports.
- `expired`: The key is not advertised to clients and will eventually be deleted.

These states are to facilitate key caching and rotation. The lifecycle of a key
is as follows:
1. The key is created in the `pending` state.
1. The in-memory caches of all Janus replicas must pick up the new key. This can
   be done by waiting or restarting the replicas.
1. The key is moved to the `active` state.
1. Caches must reload or the application can be restarted. The pending key already
   being in-memory ensures that replicas that haven't had their advertisement
   cache reloaded can still use the pending key to decrypt reports.
1. The key operates dutifully for however long the key rotation interval is.
1. When the key is due for expiry, a new key is introduced using the same steps
   as above.
1. The old key is moved to the `expired` state so that it's no longer advertised
   to clients. Because clients cache the HPKE advertisement, the key must still
   be available for report decryption.
1. For system integrity and availability, the expired key should not be deleted
   until after the last client report submitted before the key was expired has
   also expired.

Note: If we're considering implementing [#1641][1], we should automate key
lifecycle.
   
## Provisioning

A key can be generated by using the Janus aggregator API.

```bash
AGGREGATOR_URL=http://localhost:8081
AGGREGATOR_API_TOKEN="BASE64URL UNPADDED TOKEN HERE"

curl -v -X PUT \
    -H "Authorization: Bearer $AGGREGATOR_API_TOKEN" \
    -H "Accept: application/vnd.janus.aggregator+json;version=0.1" \
    -H "Content-Type: application/vnd.janus.aggregator+json;version=0.1" \
    "$AGGREGATOR_URL/hpke_configs" \
    --data '{}'
```

Example response:
```json
{
  "config": {
    "id": 1,
    "kem_id": "X25519HkdfSha256",
    "kdf_id": "HkdfSha256",
    "aead_id": "Aes128Gcm",
    "public_key": "Q6WsU8wTEYLGaSUZ0M64osfG67AfwZBxWvXp3lxIfxQ"
  },
  "state": "pending"
}
```

The keypair and ID will be generated for you, and stored in the database. If
you need to change the ciphers used, provide the `kem_id`, `kdf_id`, `aead_id`
parameters in the request body.

The key is created in the pending state. To move it into the active state:
```bash
KEY_ID=1

curl -v -X PATCH \
    -H "Authorization: Bearer $AGGREGATOR_API_TOKEN" \
    -H "Accept: application/vnd.janus.aggregator+json;version=0.1" \
    -H "Content-Type: application/vnd.janus.aggregator+json;version=0.1" \
    "$AGGREGATOR_URL/hpke_configs/$KEY_ID" \
    --data '{"state": "active"}'
```


This should return `200 OK` on success. If you need to mark it expired, change
the request body.

Other helpful methods are as follows:
- `GET /hpke_configs`: retrieve the details about all global keys.
- `GET /hpke_configs/{:id}`: retrieve the details about a single key.
- `DELETE /hpke_configs/{:id}`: fully delete a key from the database, this is
  dangerous!

Note that the aggregator API will never directly expose the private key to you.