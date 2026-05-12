# Decorators

This page states Perry's stance on TypeScript decorators and shows the
recommended decorator-free pattern for porting Angular / NestJS / TypeORM
code.

## Stance

**Perry treats decorators as a legacy compatibility surface, not a
language primitive.** The TypeScript ecosystem has been steadily
migrating away from decorators since around 2020 — modern frameworks
like Drizzle, Hono, tRPC, Prisma, Zod, SolidJS, and Vue 3's Composition
API use plain functions and schema-as-code. Even Angular's Ivy compiler
already AOT-deletes most decorator metadata at build time, and TC39's
new stage-3 decorator spec deliberately drops the runtime type
reflection that NestJS and TypeORM rely on.

Perry follows the modern direction: types are erased at compile time
(see [Limitations](limitations.md)), there is no `Reflect.metadata`,
no `Symbol`-keyed metadata side-tables, and no runtime DI container.
Code that depends on those facilities does not run on Perry as-is and
must be migrated to one of the patterns below.

## What works today

Perry parses decorator syntax (legacy / experimental form) and supports
**compile-time-only** transforms. The bundled `@log` transform is the
canonical example — it rewrites a decorated method into a wrapper that
prints entry/exit at compile time, with zero runtime decorator
machinery. See `crates/perry-hir/src/decorator_log.rs` for the
implementation.

## What does not work

- `Reflect.metadata(...)` and `Reflect.getMetadata(...)`
- `Symbol(...)` as a metadata key
- `emitDecoratorMetadata`-style runtime type capture (constructor
  parameter types are erased; there is no `design:paramtypes`)
- Runtime DI containers that resolve dependencies by type
  (`tsyringe`, NestJS's injector, Angular's root injector)
- `class-validator`, `type-graphql`, `TypeORM` runtime metadata flows

If your code depends on any of these, the port path is *not* "wait for
Perry to add Reflect" — it is to migrate to the explicit-wiring pattern
below.

## Recommended pattern: explicit construction

The Perry-native idiom is plain classes wired together in a single
`services.ts` module in dependency order. This is how a Go or Rust
program would compose services, and it is how decorator-free TS
frameworks (Hono, tRPC servers, Drizzle apps) already work.

```typescript,no-test
// services.ts
export const api = new ApiService();
export const rating = new RatingService(api);
export const chat = new ChatService(api, rating);
```

There is no container, no `@Injectable`, no `providedIn: 'root'` —
construction order *is* the dependency graph, and it is checked by the
TypeScript compiler.

## Migration recipe: an Angular service

The example below is a real service from sharity-app
(`src/app/services/rating.service.ts`, ~80 lines), shown in its
original Angular form and ported to Perry.

### Before — Angular

```typescript,no-test
import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';
import { ApiService } from './api.service';
import { Rating } from '../models/user';

@Injectable({
  providedIn: 'root'
})
export class RatingService {
  private basePath = '/api/ratings';

  constructor(private api: ApiService) { }

  getUserRatings(userId: string): Observable<any> {
    return this.api.get(`${this.basePath}/user/${userId}`);
  }

  createRating(recipientId: string, rating: { stars: number; comment?: string }): Observable<any> {
    return this.api.post(this.basePath, {
      recipientId,
      stars: rating.stars,
      comment: rating.comment,
    });
  }

  calculateAverageRating(ratings: Rating[]): number {
    if (!ratings || ratings.length === 0) return 0;
    const sum = ratings.reduce((acc, curr) => acc + curr.rating, 0);
    return sum / ratings.length;
  }
}
```

### After — Perry

Three mechanical changes:

1. **Drop `@Injectable`.** It carried no information that the class shape
   does not already carry.
2. **Replace `Observable<T>` with `Promise<T>`** for HTTP calls. Most
   Angular Observables-from-HTTP are single-value and behave like
   Promises. (For multi-value streams, use `AsyncIterable`.)
3. **Replace constructor-parameter properties** (`private api: ApiService`)
   with explicit field declarations. Perry supports parameter
   properties, but explicit fields read more clearly when the class is
   instantiated by hand rather than by a container.

```typescript,no-test
import { ApiService } from './api.service';
import { Rating } from '../models/user';

export class RatingService {
  private basePath = '/api/ratings';
  private api: ApiService;

  constructor(api: ApiService) {
    this.api = api;
  }

  async getUserRatings(userId: string): Promise<unknown> {
    return this.api.get(`${this.basePath}/user/${userId}`);
  }

  async createRating(
    recipientId: string,
    rating: { stars: number; comment?: string },
  ): Promise<unknown> {
    return this.api.post(this.basePath, {
      recipientId,
      stars: rating.stars,
      comment: rating.comment,
    });
  }

  calculateAverageRating(ratings: Rating[]): number {
    if (!ratings || ratings.length === 0) return 0;
    const sum = ratings.reduce((acc, curr) => acc + curr.rating, 0);
    return sum / ratings.length;
  }
}
```

### Wiring

```typescript,no-test
// services.ts — single source of truth for service construction
import { ApiService } from './services/api.service';
import { RatingService } from './services/rating.service';

export const api = new ApiService();
export const rating = new RatingService(api);
```

```typescript,no-test
// any consumer
import { rating } from './services';

const avg = rating.calculateAverageRating(myRatings);
const list = await rating.getUserRatings('user-123');
```

That is the entire migration. The `@Injectable` decorator, the
`providedIn: 'root'` token, the implicit container lookup — all of it
collapses into one `new RatingService(api)` line in `services.ts`.

## What about Angular components, NestJS controllers, TypeORM entities?

Perry does not support these decorator surfaces today, and the runtime
metadata they rely on is not on the roadmap. The Path-B option of
recognizing `@Component` / `@Controller` / `@Entity` at the compiler
level (analogous to Angular Ivy's AOT step) is reserved for if and when
a concrete port needs it — see [issue #581][issue-581] for the tracking
discussion. For now, the recommendation is the same: drop the
decorator, write the equivalent explicit construction, register routes
or schema as plain function calls / module-level constants.

[issue-581]: https://github.com/PerryTS/perry/issues/581

## Future direction

If decorators come back into ecosystem fashion, it will be in the
[TC39 stage-3 form][tc39-decorators] — pure compile-time, no metadata
reflection — which aligns naturally with Perry's "types erased,
compile to native" architecture. Any future investment in decorator
support will target that spec, not the legacy / experimental form that
Angular and NestJS use today.

[tc39-decorators]: https://github.com/tc39/proposal-decorators
