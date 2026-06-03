# DeepWiki Response

Repository: `tc39/ecma262`
Question: For ECMAScript assignment evaluation, what is the specified order for evaluating computed property assignment targets such as obj[expr] = rhs, including when base/key/RHS are evaluated and when PutValue occurs? For object destructuring assignment with computed property names and keyed member targets such as ({ [keyExpr]: targetObj[targetKeyExpr] } = source), what is the order of evaluating source property keys versus assignment target references? Please point to the relevant ecma262 algorithms/sections.
Search: https://deepwiki.com/search/for-ecmascript-assignment-eval_e2179d67-bd2e-4279-91a3-ee661ac5f316

**Question**: For ECMAScript assignment evaluation, what is the specified order for evaluating computed property assignment targets such as obj[expr] = rhs, including when base/key/RHS are evaluated and when PutValue occurs? For object destructuring assignment with computed property names and keyed member targets such as ({ [keyExpr]: targetObj[targetKeyExpr] } = source), what is the order of evaluating source property keys versus assignment target references? Please point to the relevant ecma262 algorithms/sections.

The provided codebase context primarily concerns the TC39 proposal process and the structure of the `ecma262` repository [1](#ref-q1-1)  [2](#ref-q1-2)  [3](#ref-q1-3) . It describes how new ECMAScript features are proposed, developed, and integrated into the specification [4](#ref-q1-4) .

However, the context does not contain the actual ECMAScript specification text or algorithms that detail the evaluation order for computed property assignments or object destructuring assignments . Therefore, I cannot point to specific `ecma262` algorithms or sections to answer your question about the evaluation order of `obj[expr] = rhs` or `({ [keyExpr]: targetObj[targetKeyExpr] } = source)` .

To get this information, you would typically need to consult the official ECMAScript Language Specification, which is a separate document that defines the precise semantics of JavaScript [5](#ref-q1-5) .

## Notes
The provided context focuses on the meta-process of ECMAScript development rather than the technical details of the language specification itself [1](#ref-q1-1)  [6](#ref-q1-6) . Files like `FAQ.md`, `README.md`, and `CONTRIBUTING.md` explain how to contribute to the specification and the proposal stages [4](#ref-q1-4)  [7](#ref-q1-7) . The workflow files (`.github/workflows/publish-biblio.yml`, `.github/workflows/preview.yml`) are related to publishing and previewing the specification document, not its content [8](#ref-q1-8)  [9](#ref-q1-9) .

Wiki pages you might want to explore:
- [TC39 Proposal Process (tc39/ecma262)](/wiki/tc39/ecma262#3)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-assignment-eval_e2179d67-bd2e-4279-91a3-ee661ac5f316

## References

<a id="ref-q1-1"></a>
### [1] `FAQ.md:1-102`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/FAQ.md#L1-L102)

```markdown
# Frequently Asked Questions

An index of frequently asked questions regarding all things ECMA-262.

# Process Questions

##### What is the process for proposing a new feature?

New features start life as a proposal to the [TC39](#what-is-a-tc39) committee and must be championed (or co-championed) by at least one member of the committee. Once the proposal is raised at a committee meeting, it will become a Stage 0 proposal and move along from there. For more details on how proposal stages work, check out the [proposal process document][proposal-process-document].

If you would like to contribute, please check out [Contributing to ECMAScript](https://github.com/tc39/ecma262/blob/HEAD/CONTRIBUTING.md).

##### What is a "TC39"?

TC39 stands for "Technical Committee 39" and is the committee responsible for iterating on and evolving the ECMAScript language specification. The committee generally meets around 6 times a year to discuss progress on pending proposals and collectively work on moving forward with changes to the spec.

##### Why can't we remove feature X?

Changes to ECMAScript must carefully consider the state of the world using the previous version of the language. This includes a large percentage of the web. As a result, in order to remove a feature from ECMAScript, TC39 must be able to show that the feature is used almost never (and thus can be removed). Going through this exercise is extremely difficult and sometimes impossible -- so in general ECMAScript *very* rarely removes features.

Because the web is so large, even features that behave in a way that's surprising and potentially lead to bugs are often relied upon by real programs. Therefore, only actual use data, and not a sense of whether some feature is correct or useful, can guide TC39 in potentially changing existing behavior.

# Feature Questions

### Arrow Functions

##### Why isn't there a `->` version of arrow functions?

The motivation for `=>` was to address the oft-fired footgun of dynamic `this` bindings. Additionally, having two forms of arrows is confusing; So only one form was added.

### Destructuring

##### Why isn't the object property destructuring syntax flipped the other way?

(i.e. `let {x: y} = {x: 42}` vs `let {y: x} = {x: 42}`)

In all other object patterns in the language, the syntax to the left of the colon represents the "structure" of an object; So having destructuring patterns match this convention was most consistent.

More fundamentally, however, flipping the syntax the other way would produce a grammar that requires infinite lookahead to properly disambiguate.

### Modules

##### Why don't `import` statements use real destructuring syntax?

[`import` statements create an alias of a remote binding](#why-are-imported-module-bindings-aliased-instead-of-copied), they do not create a new local binding. First-class destructuring, however, allows for the creation of new bindings from substructures of objects and arrays. As a result first-class destructuring was not a good fit for the `import` statement.

##### Why are imported module bindings aliased instead of copied?

The biggest reason for this is that it allows cyclic module dependencies to work.

For example, consider the following contrived scenario:

```javascript
// Even.js
import {isOdd} from "./Odd.js";

export function isEven(num) {
  if (num === 0) {
    return true;
  } else {
    return isOdd(num - 1);
  }
}
```

```javascript
// Odd.js
import {isEven} from "./Even.js";

export function isOdd(num) {
  if (num === 0) {
    return false;
  } else {
    return isEven(num - 1);
  }
}
```

```javascript
// main.js
import {isOdd} from "./Odd";

isOdd(2);
```

The list of operations that execute will go something like the following:

1. Note that **main.js** has a named import called `isOdd` that comes from **Odd.js**
2. Begin loading **Odd.js**.
3. Once **Odd.js** has loaded, note that it has a named export called `isOdd` and a named import called `isEven` that comes from **Even.js**.
4. Create an empty binding called `isOdd` for **Odd.js**'s exports.
5. Begin loading **Even.js**.
6. Once **Even.js** has loaded, note that it has a named export called `isEven` and a named import called `isOdd` that comes from **Odd.js**.
7. Create an empty binding called `isEven` for **Even.js**'s exports.
8. Now that all of the dependencies of **Even.js** have loaded, begin evaluating it with a variable called `isOdd` aliased to the (currently empty) `isOdd` binding we created in step 4.
9. As we evaluate the `export function isEven() { ... }` statement in **Even.js**, fill in the value for the `isEven` binding created in step 7.
10. Now that all of the dependencies of **Odd.js** have loaded, begin evaluating it with a variable called `isEven` aliased to the (no longer empty) `isEven` binding we created in step 9.
11. As we evaluate the `export function isOdd() { ... }` statement in **Odd.js**, fill in the value for the `isOdd` binding created in step 4. Note that this now "fills in" the value for the alias to this binding noted in step 8.

If the exported bindings were copied between **Even.js** and **Odd.js** rather than aliased, the body of `isEven` would have received a copy of the uninitialized value for `isOdd`.

[proposal-process-document]: https://tc39.es/process-document/
```

<a id="ref-q1-2"></a>
### [2] `README.md:1-36`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/README.md#L1-L36)

```markdown

ECMAScript
====

## This repo

This repository contains the source for the current draft of ECMA-262,
the ECMAScript® Language Specification.

This source is processed to obtain a human-readable version,
which you can view [here](https://tc39.es/ecma262/).

If you want to explore how the specification was written, you can also view the source with its history in [searchfox](https://searchfox.org/ecma262/source/spec.html).

## Current Proposals

Proposals follow [the TC39 process](https://tc39.es/process-document/) and are tracked in the [proposals repository](https://github.com/tc39/proposals).

* [Finished Proposals](https://github.com/tc39/proposals/blob/HEAD/finished-proposals.md)
* [Active Proposals](https://github.com/tc39/proposals)
* [Stage 1 Proposals](https://github.com/tc39/proposals/blob/HEAD/stage-1-proposals.md)
* [Stage 0 Proposals](https://github.com/tc39/proposals/blob/HEAD/stage-0-proposals.md)
* [Inactive Proposals](https://github.com/tc39/proposals/blob/HEAD/inactive-proposals.md)

### Contributing New Proposals

Please see [Contributing to ECMAScript](/CONTRIBUTING.md) for the most up-to-date information on contributing proposals to this standard.

## Developing the Specification

After cloning, do `npm install` to set up your environment. You can then do `npm run build` to build the spec or `npm run watch` to set up a continuous build. The results will appear in the `out` directory, which you can use `npm run clean` to delete.

## Community

* [ES discourse](https://es.discourse.group/): Forum for ECMAScript discussion and questions
* [Matrix](https://github.com/tc39/how-we-work/blob/HEAD/matrix-guide.md): Chat
```

<a id="ref-q1-3"></a>
### [3] `CONTRIBUTING.md:31-77`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/CONTRIBUTING.md#L31-L77)

```markdown
* [ECMAScript Intl API](https://tc39.es/ecma402/) - [file an issue](https://github.com/tc39/ecma402/issues/new)
* [WebAssembly](https://webassembly.github.io/spec/) - [file an issue](https://github.com/WebAssembly/spec/issues/new)

## New feature proposals

TC39 is open to accepting new feature requests for ECMAScript, referred to as "proposals". Proposals go through a four-stage process which is documented in the [TC39 process document](https://tc39.es/process-document/).

Feature requests for future versions of ECMAScript should not be made in this repository. Instead, they are developed in separate GitHub repositories, which are then merged into the main repository once they have received "Stage 4".

### Creating a new proposal

To make a feature request, document the problem and a sketch of the solution with others in the community, including TC39 members. One place to do this is the [TC39 Discourse](https://es.discourse.group/); another is the [Matrix chat room][].

Your goal will be to convince others that your proposal is a useful addition to the language and recruit TC39 members to help turn your request into a proposal and shepherd it into the language. Once a proposal is introduced to the committee, new features are considered by the committee according to the [TC39 process document](https://tc39.es/process-document/).

You can look at [existing proposals](https://github.com/tc39/proposals/) for examples of how proposals are structured, and some delegates use [this template](https://github.com/tc39/template-for-proposals) when creating repositories for their proposals. Proposals need to have a repository and be moved to the TC39 org on GitHub once they reach Stage 1.

### TC39 meetings and champions

If you have a new proposal you want to get into the language, you first need a TC39 "champion": a member of the committee who will make the case for the proposal at [in-person TC39 meetings](https://github.com/tc39/agendas#agendas) and help it move through the process. If you are a TC39 member, you can be a champion; otherwise, find a TC39 member to work with for this (e.g., through the [TC39 discussion group](https://es.discourse.group/) or the [Matrix chat room][]). Proposals may have multiple champions (a "champion group").

TC39 meets six times a year, mostly in the United States, to discuss proposals. It is possible for members to join meetings remotely. At meetings, we discuss ways to resolve issues and feature requests. We spend most of the time considering proposals and advancing them through the stage process. Meetings follow an agenda which is developed in the [agendas GitHub repository](https://github.com/tc39/agendas/). After the meeting, notes are published in the [notes GitHub repository](https://github.com/tc39/tc39-notes/). To advance your proposal towards inclusion in the final specification, ensure that it is included on the agenda for an upcoming meeting and propose advancement at that time.

### Helping with existing proposals

TC39 is currently considering adding several new features to the language. These proposals are linked from [the proposals repository](https://github.com/tc39/proposals). There are many ways to help with existing proposals:
  * File issues in the individual proposal repository to provide constructive criticism and feedback.
  * Make PRs against proposals, e.g., to clarify explanations of the motivation and use cases in `README.md`, or to fix issues in the proposal's specification text.
  * Talk about what you think of the proposal, including sharing thoughts with the champion.
  * Blog, tweet, give talks, etc about proposals to get more awareness and programmer feedback about them.
  * Write [test262](https://github.com/tc39/test262) tests against the proposal, which are used to verify implementations. (If the proposal is at Stage 3, the tests can land; if earlier, they can be maintained in a PR.)
  * Implement or prototype the proposal in script engines, parsers, transpilers, polyfills, type checkers, etc., possibly behind a flag or in a separate module, and report feedback. Note that proposals before Stage 3 are highly unstable, and all proposals before Stage 4 may be modified or dropped.

To track what's going on with a particular proposal, you can look in issues and commits in the individual proposal repository, read presentation slides which are linked from the TC39 agenda, and read the notes which came from the subsequent meetings. You can also reach out via the [Matrix chat room][], the [discussion group](https://es.discourse.group/), or direct communication with a proposal champion, if the other resources are unclear.

## Required legal agreements

People associated with Ecma member organizations have a legal agreement in place with Ecma to ensure that intellectual property rights (IPR) of their contributions are appropriately licensed to be available to all ECMAScript programmers and implementers. For non-members to contribute, you are required to make these rights available by signing a Contributor License Agreement (CLA) for non-trivial contributions.

If you wish to submit a proposal or make a significant PR, and you are not a representative of a TC39 member, please [register as a TC39 RFTG contributor](https://tc39.es/agreements/contributor/).

Ecma TC39 accepts contributions from non-member individuals who have accepted the TC39 copyright and patent policies. Currently all ECMAScript related technical work is done by the TC39 RF TG (Royalty Free Task Group), for which the following IPR policies apply:

  * [Ecma International RF Patent Policy](https://ecma-international.org/memento/Policies/Ecma_Royalty-Free_Patent_Policy_Extension_Option.htm)
  * [Ecma International Software Copyright Policy](https://ecma-international.org/memento/Policies/Ecma_Policy_on_Submission_Inclusion_and_Licensing_of_Software.htm) ([PDF](https://ecma-international.org/memento/Policies/Ecma_Policy_on_Submission_Inclusion_and_Licensing_of_Software.pdf))

[Matrix chat room]: https://github.com/tc39/how-we-work/blob/HEAD/matrix-guide.md
```

<a id="ref-q1-4"></a>
### [4] `FAQ.md:7-10`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/FAQ.md#L7-L10)

```markdown
##### What is the process for proposing a new feature?

New features start life as a proposal to the [TC39](#what-is-a-tc39) committee and must be championed (or co-championed) by at least one member of the committee. Once the proposal is raised at a committee meeting, it will become a Stage 0 proposal and move along from there. For more details on how proposal stages work, check out the [proposal process document][proposal-process-document].
```

<a id="ref-q1-5"></a>
### [5] `README.md:7-8`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/README.md#L7-L8)

```markdown
This repository contains the source for the current draft of ECMA-262,
the ECMAScript® Language Specification.
```

<a id="ref-q1-6"></a>
### [6] `CONTRIBUTING.md:34-37`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/CONTRIBUTING.md#L34-L37)

```markdown
## New feature proposals

TC39 is open to accepting new feature requests for ECMAScript, referred to as "proposals". Proposals go through a four-stage process which is documented in the [TC39 process document](https://tc39.es/process-document/).
```

<a id="ref-q1-7"></a>
### [7] `CONTRIBUTING.md:36-37`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/CONTRIBUTING.md#L36-L37)

```markdown
TC39 is open to accepting new feature requests for ECMAScript, referred to as "proposals". Proposals go through a four-stage process which is documented in the [TC39 process document](https://tc39.es/process-document/).
```

<a id="ref-q1-8"></a>
### [8] `.github/workflows/publish-biblio.yml:1-32`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/.github/workflows/publish-biblio.yml#L1-L32)

```
name: 'ecma-262-biblio'

on:
  push:
    branches:
      - main

jobs:
  publish:
    name: 'publish ecma262-biblio'
    runs-on: ubuntu-22.04
    if: ${{ github.repository == 'tc39/ecma262' }}

    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Setup node
        uses: actions/setup-node@v4
        with:
          node-version: 'lts/*'
          registry-url: 'https://registry.npmjs.org'

      - name: Install dependencies
        run: npm ci --no-audit

      - name: Publish biblio
        run: scripts/publish-biblio.sh
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN_FOR_TC39_USER }}
```

<a id="ref-q1-9"></a>
### [9] `.github/workflows/preview.yml:31-61`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/.github/workflows/preview.yml#L31-L61)

```
        with:
          script: |
            const { owner, repo } = context.repo;
            const artifacts = await github.rest.listWorkflowRunArtifacts({
               owner,
               repo,
               run_id: ${{ github.event.workflow_run.id }},
            });
            const { id: artifact_id } = artifacts.data.artifacts.find((artifact) => artifact.name == 'out');
            const download = await github.rest.downloadArtifact({
               owner,
               repo,
               artifact_id,
               archive_format: 'zip',
            });
            const fs = require('fs');
            fs.writeFileSync('${{ github.workspace }}/out.zip', Buffer.from(download.data));
      - run: unzip -o out.zip -d out
      - name: 'debug info'
        uses: actions/github-script@v7
        with:
          script: |
            console.log(${{ toJson(github.event) }});
      - run: head -n1 out/pr.txt | grep -e '^[1-9][0-9]*$'
        name: 'validate PR number from archive'
      - run: echo "PULL_REQUEST=$(head -n1 out/pr.txt)" >> $GITHUB_ENV
      - run: rm out/pr.txt && echo $PULL_REQUEST
      - run: node scripts/publish-preview
        env:
          CI_PREVIEW_TOKEN: ${{ secrets.CI_PREVIEW_TOKEN }}
          GITHUB_HEAD_SHA: ${{ github.event.workflow_run.head_commit.id }}
```
