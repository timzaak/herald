/**
 * WebAuthn passkey LOGIN assertion helper (design §5.4).
 *
 * Only `navigator.credentials.get` (assertion) — no registration/create. The
 * SDK never manages authenticators (DEC-js-sdk-001). The integrator passes the
 * server-provided options to `performPasskeyAssertion` and submits the returned
 * assertion to `passkey.loginFinish`.
 *
 * base64url encode/decode is implemented on native `ArrayBuffer` ↔ `string`
 * (no dependency).
 */

/** Server-provided WebAuthn request options, JSON-encoded (base64url fields). */
export interface PublicKeyCredentialRequestOptionsJSON {
  challenge: string
  rpId?: string
  timeout?: number
  userVerification?: UserVerificationRequirement
  allowCredentials?: Array<{
    type: 'public-key'
    id: string
    transports?: AuthenticatorTransport[]
  }>
}

/** Assertion result ready to submit to the passkey verify endpoint. */
export interface AssertionResultJSON {
  id: string
  rawId: string
  type: 'public-key'
  response: {
    authenticatorData: string
    clientDataJSON: string
    signature: string
    userHandle?: string | null
  }
  clientExtensionResults?: Record<string, unknown>
}

// --- base64url codec (exported for unit testing) ---

/** Decode an unpadded base64url string to an `ArrayBuffer`. */
export function base64urlToBuffer(input: string): ArrayBuffer {
  const b64 = input.replace(/-/g, '+').replace(/_/g, '/')
  // Re-pad to a multiple of 4.
  const padded = b64 + '==='.slice((b64.length + 3) % 4)
  const binary = atob(padded)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes.buffer
}

/** Encode an `ArrayBuffer` / `Uint8Array` as an unpadded base64url string. */
export function bufferToBase64url(buffer: ArrayBuffer | Uint8Array): string {
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer)
  let binary = ''
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i] ?? 0)
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/[=]/g, '')
}

/**
 * Perform a WebAuthn assertion for passkey login.
 *
 * @throws when WebAuthn is unavailable or the user cancels.
 */
export async function performPasskeyAssertion(
  options: PublicKeyCredentialRequestOptionsJSON,
): Promise<AssertionResultJSON> {
  if (typeof navigator === 'undefined' || !navigator.credentials?.get) {
    throw new Error('WebAuthn (navigator.credentials.get) is not available in this environment.')
  }

  const publicKey: PublicKeyCredentialRequestOptions = {
    challenge: base64urlToBuffer(options.challenge),
    ...(options.rpId ? { rpId: options.rpId } : {}),
    ...(options.timeout !== undefined ? { timeout: options.timeout } : {}),
    ...(options.userVerification ? { userVerification: options.userVerification } : {}),
    ...(options.allowCredentials
      ? {
          allowCredentials: options.allowCredentials.map((c) => ({
            type: 'public-key' as const,
            id: base64urlToBuffer(c.id),
            ...(c.transports ? { transports: c.transports } : {}),
          })),
        }
      : {}),
  }

  const credential = (await navigator.credentials.get({ publicKey })) as PublicKeyCredential | null
  if (!credential) {
    throw new Error('Passkey assertion returned no credential.')
  }

  const response = credential.response as AuthenticatorAssertionResponse
  const result: AssertionResultJSON = {
    id: credential.id,
    rawId: bufferToBase64url(credential.rawId),
    type: 'public-key',
    response: {
      authenticatorData: bufferToBase64url(response.authenticatorData),
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      signature: bufferToBase64url(response.signature),
      ...(response.userHandle ? { userHandle: bufferToBase64url(response.userHandle) } : {}),
    },
  }

  // `getClientExtensionResults` is synchronous per the WebAuthn spec. The
  // result is serialized opaquely to the backend (the verify body holds the
  // assertion as `unknown`), so cast to the JSON record shape here.
  if (typeof credential.getClientExtensionResults === 'function') {
    result.clientExtensionResults =
      credential.getClientExtensionResults() as unknown as Record<string, unknown>
  }

  return result
}
