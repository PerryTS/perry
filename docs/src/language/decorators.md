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

Perry still follows the modern direction: types are erased at compile
time (see [Limitations](limitations.md)) and there is no runtime DI
container. A small legacy compatibility path exists for libraries that
only need AOT-lowerable decorator side effects and metadata.
Code that depends on richer decorator behavior still needs one of the
patterns below.

## What works today

Perry parses legacy / experimental TypeScript decorator syntax and
supports two paths:

- **Legacy class decorators, method decorators, property decorators,
  constructor parameter decorators, and method parameter decorators** for
  Nest-style DI and route metadata canaries. Decorator functions run for
  side effects, `Reflect.defineMetadata`, `Reflect.getMetadata`,
  `Reflect.getOwnMetadata`, `Reflect.hasMetadata`,
  `Reflect.hasOwnMetadata`, `Reflect.getMetadataKeys`,
  `Reflect.getOwnMetadataKeys`, `Reflect.deleteMetadata`, and
  `@Reflect.metadata(...)` are available. Perry emits
  `design:paramtypes` for decorated classes/methods and `design:type`
  for decorated properties.
- **Compile-time-only transforms.** The bundled `@log` transform is the
  canonical example — it rewrites a decorated method into a wrapper that
  prints entry/exit at compile time, with zero runtime decorator
  machinery. See `crates/perry-hir/src/decorator_log.rs` for the
  implementation.

## What does not work

- Accessor decorators and descriptor replacement
- Decorator class replacement return values. If a class decorator
  returns anything other than `undefined`, Perry throws a `TypeError`
  at decorator application time. Real-world decorators like
  `@Memoize`, `@Throttle`, and GraphQL resolver wrappers that return
  wrapped classes need a Perry-aware port — the lowered class is fixed
  in the IR and cannot be replaced at runtime.
- General `Reflect.metadata(...)` helper calls outside decorator syntax
- `Symbol(...)` as a metadata key
- `emitDecoratorMetadata` beyond class/method `design:paramtypes` and
  property `design:type`
- Runtime DI containers that resolve dependencies by type
  beyond the reduced class-constructor canary (`tsyringe`, full NestJS
  injector behavior, Angular's root injector)
- `class-validator`, `type-graphql`, `TypeORM` runtime metadata flows

If your code depends on any of these, the port path is still explicit
wiring or a dedicated AOT transform, not relying on the full legacy
TypeScript decorator runtime.

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

Perry's reduced legacy path is enough for small Nest-style
constructor-injection and route-metadata canaries, but it is not full
Angular, NestJS, or TypeORM compatibility. The Path-B option of
recognizing `@Component` / `@Controller` / `@Entity` at the compiler
level (analogous to Angular Ivy's AOT step) is reserved for if and when
a concrete port needs it — see [issue #581][issue-581] for the tracking
discussion. For now, the recommendation is the same: drop the decorator
where possible, write the equivalent explicit construction, register
routes or schema as plain function calls / module-level constants.

[issue-581]: https://github.com/PerryTS/perry/issues/581

## Future direction

New feature work should prefer the [TC39 stage-3 form][tc39-decorators]
because it aligns better with Perry's "types erased, compile to native"
architecture. The legacy TypeScript path exists for compatibility and
will stay focused on narrow AOT-lowerable metadata cases rather than
becoming a full `tsc` decorator runtime.

[tc39-decorators]: https://github.com/tc39/proposal-decorators
