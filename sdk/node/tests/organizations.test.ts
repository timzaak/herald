/**
 * Organization authorization endpoint coverage, 1:1 with the Rust `herald-sdk`
 * crate (`sdk/rust/src/lib.rs`) and `backend/api-ext/src/organization.rs`.
 *
 * WHY: downstream apps rely on exact paths, camelCase payloads, 204 empty-body
 * deletes, query encoding of `actorUserId`, and non-member vs denied-member
 * response shapes — any drift silently breaks multi-tenant workspace access
 * control at runtime.
 */

import { describe, expect, it } from 'vitest'
import { http, HttpResponse } from 'msw'
import { HeraldClient, HeraldSdkError } from '../src'
import { server } from './mocks/server'
import { API_KEY, BASE_URL } from './helpers'

const client = () => new HeraldClient(BASE_URL, API_KEY)

const REALM_ID = 'realm-001'
const ORG_ID = '00000000-0000-0000-0000-000000000001'
const USER_ID = '00000000-0000-0000-0000-000000000002'
const ACTOR_ID = '00000000-0000-0000-0000-000000000003'

const orgResponse = {
  id: ORG_ID,
  externalId: 'ws-42',
  name: 'Acme Workspace',
  ownerId: USER_ID,
  realmId: REALM_ID,
  clientId: 'client-abc',
  createdAt: '2026-10-13T00:00:00Z',
  created: true,
}

describe('organizations', () => {
  it('createOrg posts camelCase payload and handles 201 create vs 200 idempotent replay', async () => {
    const bodies: Array<Record<string, unknown>> = []
    let calls = 0
    server.use(
      http.post(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs`, async ({ request }) => {
        bodies.push((await request.json()) as Record<string, unknown>)
        calls += 1
        return HttpResponse.json(
          { ...orgResponse, created: calls === 1 },
          { status: calls === 1 ? 201 : 200 },
        )
      }),
    )

    const first = await client().createOrg(REALM_ID, {
      externalId: 'ws-42',
      name: 'Acme Workspace',
      ownerId: USER_ID,
    })
    const replay = await client().createOrg(REALM_ID, {
      externalId: 'ws-42',
      name: null,
      ownerId: USER_ID,
    })

    expect(bodies[0]).toEqual({
      externalId: 'ws-42',
      name: 'Acme Workspace',
      ownerId: USER_ID,
    })
    expect(first.created).toBe(true)
    expect(replay.created).toBe(false)
    expect(replay.id).toBe(ORG_ID)
  })

  it('deleteOrg resolves on 204 No Content and maps repeated 404 to not-found', async () => {
    let calls = 0
    server.use(
      http.delete(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}`, () => {
        calls += 1
        if (calls === 1) return new HttpResponse(null, { status: 204 })
        return HttpResponse.json({ code: 'org_not_found' }, { status: 404 })
      }),
    )

    await expect(client().deleteOrg(REALM_ID, ORG_ID)).resolves.toBeUndefined()

    const error = await client().deleteOrg(REALM_ID, ORG_ID).catch((e: unknown) => e)
    expect(error).toBeInstanceOf(HeraldSdkError)
    expect((error as HeraldSdkError).code).toBe('not-found')
    expect((error as HeraldSdkError).status).toBe(404)
  })

  it('upsertOrgMember sends actorUserId when provided and surfaces 403 anti-escalation errors', async () => {
    let capturedBody: Record<string, unknown> | undefined
    server.use(
      http.post(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/members`, async ({ request }) => {
        capturedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({
          orgId: ORG_ID,
          userId: USER_ID,
          role: { id: 'role-admin', name: 'org-admin' },
          isOwner: false,
          createdAt: '2026-10-13T00:00:00Z',
          updatedAt: '2026-10-13T01:00:00Z',
        })
      }),
    )

    const member = await client().upsertOrgMember(REALM_ID, ORG_ID, {
      userId: USER_ID,
      role: 'org-admin',
      actorUserId: ACTOR_ID,
    })

    expect(capturedBody).toEqual({
      userId: USER_ID,
      role: 'org-admin',
      actorUserId: ACTOR_ID,
    })
    expect(member.role.name).toBe('org-admin')
    expect(member.isOwner).toBe(false)

    server.use(
      http.post(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/members`, () =>
        HttpResponse.json({ code: 'org_actor_self_hold_required' }, { status: 403 }),
      ),
    )
    const err = await client()
      .upsertOrgMember(REALM_ID, ORG_ID, { userId: USER_ID, role: 'org-owner', actorUserId: ACTOR_ID })
      .catch((e: unknown) => e)
    expect(err).toBeInstanceOf(HeraldSdkError)
    expect((err as HeraldSdkError).code).toBe('forbidden')
  })

  it('removeOrgMember URL-encodes actorUserId and omits query param when absent', async () => {
    const capturedUrls: string[] = []
    server.use(
      http.delete(
        `${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/members/${USER_ID}`,
        ({ request }) => {
          capturedUrls.push(request.url)
          return new HttpResponse(null, { status: 204 })
        },
      ),
    )

    // '#' would truncate the query into a fragment and '&' would spawn a stray
    // parameter if concatenated unencoded (mirrors Rust test_remove_org_member_encodes_actor_user_id).
    await client().removeOrgMember(REALM_ID, ORG_ID, USER_ID, 'actor#with&reserved+chars')
    await client().removeOrgMember(REALM_ID, ORG_ID, USER_ID)

    const withActor = new URL(capturedUrls[0]!)
    expect(withActor.searchParams.get('actorUserId')).toBe('actor#with&reserved+chars')

    const withoutActor = new URL(capturedUrls[1]!)
    expect(withoutActor.searchParams.has('actorUserId')).toBe(false)
  })

  it('removeOrgMember maps 409 owner/last-admin protection to api-error', async () => {
    server.use(
      http.delete(
        `${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/members/${USER_ID}`,
        () => HttpResponse.json({ code: 'org_owner_protected' }, { status: 409 }),
      ),
    )

    const err = await client().removeOrgMember(REALM_ID, ORG_ID, USER_ID).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(HeraldSdkError)
    expect((err as HeraldSdkError).code).toBe('api-error')
    expect((err as HeraldSdkError).status).toBe(409)
  })

  it('listOrgMembers and listOrgMembersPage pass pagination params and return OrgMemberPage', async () => {
    const capturedUrls: string[] = []
    server.use(
      http.get(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/members`, ({ request }) => {
        capturedUrls.push(request.url)
        const url = new URL(request.url)
        return HttpResponse.json({
          items: [
            {
              userId: USER_ID,
              email: 'alice@example.com',
              nickname: null,
              role: { id: 'role-owner', name: 'org-owner' },
              isOwner: true,
              joinedAt: '2026-10-13T00:00:00Z',
            },
          ],
          page: Number(url.searchParams.get('page') ?? 1),
          pageSize: Number(url.searchParams.get('pageSize') ?? 20),
          total: 1,
        })
      }),
    )

    const defaultPage = await client().listOrgMembers(REALM_ID, ORG_ID)
    const paged = await client().listOrgMembersPage(REALM_ID, ORG_ID, 2, 50)

    expect(new URL(capturedUrls[0]!).search).toBe('')
    expect(defaultPage.items).toHaveLength(1)
    expect(defaultPage.items[0]?.nickname).toBeNull()
    expect(defaultPage.total).toBe(1)

    const pagedUrl = new URL(capturedUrls[1]!)
    expect(pagedUrl.searchParams.get('page')).toBe('2')
    expect(pagedUrl.searchParams.get('pageSize')).toBe('50')
    expect(paged.page).toBe(2)
    expect(paged.pageSize).toBe(50)
  })

  it('getOrgMember distinguishes member details from non-member lookup', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/members/${USER_ID}`, () =>
        HttpResponse.json({
          member: true,
          userId: USER_ID,
          role: { id: 'role-member', name: 'org-member' },
          isOwner: false,
          joinedAt: '2026-10-13T00:00:00Z',
        }),
      ),
      http.get(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/members/outsider`, () =>
        HttpResponse.json({
          member: false,
          userId: 'outsider',
        }),
      ),
    )

    const member = await client().getOrgMember(REALM_ID, ORG_ID, USER_ID)
    expect(member.member).toBe(true)
    expect(member.role?.name).toBe('org-member')
    expect(member.isOwner).toBe(false)

    const outsider = await client().getOrgMember(REALM_ID, ORG_ID, 'outsider')
    expect(outsider.member).toBe(false)
    expect(outsider.role).toBeUndefined()
    expect(outsider.isOwner).toBeUndefined()
    expect(outsider.joinedAt).toBeUndefined()
  })

  it('checkOrgPermission returns structured results for allowed, denied member, and non-member', async () => {
    const bodies: Array<Record<string, unknown>> = []
    server.use(
      http.post(
        `${BASE_URL}/api/ext/realms/${REALM_ID}/orgs/${ORG_ID}/permission-check`,
        async ({ request }) => {
          const body = (await request.json()) as Record<string, unknown>
          bodies.push(body)
          if (body.userId === USER_ID) {
            return HttpResponse.json({
              member: true,
              allowed: true,
              role: { id: 'role-admin', name: 'org-admin' },
            })
          }
          if (body.userId === 'member-denied') {
            return HttpResponse.json({
              member: true,
              allowed: false,
              role: { id: 'role-member', name: 'org-member' },
            })
          }
          return HttpResponse.json({
            member: false,
            allowed: false,
          })
        },
      ),
    )

    const rules = [{ resource: 'project', action: 'delete' }]
    const allowed = await client().checkOrgPermission(REALM_ID, ORG_ID, { userId: USER_ID, rules })
    const denied = await client().checkOrgPermission(REALM_ID, ORG_ID, { userId: 'member-denied', rules })
    const nonMember = await client().checkOrgPermission(REALM_ID, ORG_ID, { userId: 'outsider', rules })

    expect(bodies[0]).toEqual({ userId: USER_ID, rules })
    expect(allowed).toEqual({
      member: true,
      allowed: true,
      role: { id: 'role-admin', name: 'org-admin' },
    })
    expect(denied.member).toBe(true)
    expect(denied.allowed).toBe(false)
    expect(denied.role?.name).toBe('org-member')
    expect(nonMember.member).toBe(false)
    expect(nonMember.allowed).toBe(false)
    expect(nonMember.role).toBeUndefined()
  })

  it('listUserOrgs and listUserOrgsPage pass userId and pagination query params', async () => {
    const capturedUrls: string[] = []
    server.use(
      http.get(`${BASE_URL}/api/ext/realms/${REALM_ID}/orgs`, ({ request }) => {
        capturedUrls.push(request.url)
        return HttpResponse.json({
          items: [
            {
              orgId: ORG_ID,
              externalId: 'ws-42',
              name: 'Acme Workspace',
              role: { id: 'role-owner', name: 'org-owner' },
              isOwner: true,
              joinedAt: '2026-10-13T00:00:00Z',
            },
          ],
          page: 1,
          pageSize: 10,
          total: 1,
        })
      }),
    )

    const defaultPage = await client().listUserOrgs(REALM_ID, USER_ID)
    const paged = await client().listUserOrgsPage(REALM_ID, USER_ID, 3, 10)

    const firstUrl = new URL(capturedUrls[0]!)
    expect(firstUrl.searchParams.get('userId')).toBe(USER_ID)
    expect(firstUrl.searchParams.has('page')).toBe(false)
    expect(defaultPage.items[0]?.externalId).toBe('ws-42')

    const secondUrl = new URL(capturedUrls[1]!)
    expect(secondUrl.searchParams.get('userId')).toBe(USER_ID)
    expect(secondUrl.searchParams.get('page')).toBe('3')
    expect(secondUrl.searchParams.get('pageSize')).toBe('10')
    expect(paged.total).toBe(1)
  })
})
