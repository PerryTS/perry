# DeepWiki Response

Repository: `tc39/ecma262`
Question: For ECMAScript arrow functions, what does ECMA-262 specify about (1) restricted function properties caller and arguments, (2) lexical capture of new.target, and (3) lexical super property references, including how these differ from ordinary functions and which abstract operations or environment records are involved?
Search: https://deepwiki.com/search/for-ecmascript-arrow-functions_895f0aea-47e2-4bf0-b82a-b75ed9b8020a

**Question**: For ECMAScript arrow functions, what does ECMA-262 specify about (1) restricted function properties caller and arguments, (2) lexical capture of new.target, and (3) lexical super property references, including how these differ from ordinary functions and which abstract operations or environment records are involved?

I cannot answer your question about the specifics of ECMAScript arrow functions as defined by ECMA-262 regarding `caller`, `arguments`, `new.target`, and `super` properties. The provided codebase context primarily focuses on the TC39 proposal process and repository structure [1](#ref-q1-1)  [2](#ref-q1-2) , rather than the detailed technical specifications of language features within ECMA-262 itself. Therefore, I do not have the necessary information to describe how these properties are handled in arrow functions or the abstract operations and environment records involved.

Wiki pages you might want to explore:
- [TC39 Proposal Process (tc39/ecma262)](/wiki/tc39/ecma262#3)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-arrow-functions_895f0aea-47e2-4bf0-b82a-b75ed9b8020a

## References

<a id="ref-q1-1"></a>
### [1] `FAQ.md:7-10`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/FAQ.md#L7-L10)

```markdown
##### What is the process for proposing a new feature?

New features start life as a proposal to the [TC39](#what-is-a-tc39) committee and must be championed (or co-championed) by at least one member of the committee. Once the proposal is raised at a committee meeting, it will become a Stage 0 proposal and move along from there. For more details on how proposal stages work, check out the [proposal process document][proposal-process-document].
```

<a id="ref-q1-2"></a>
### [2] `CONTRIBUTING.md:36-39`
Source: [tc39/ecma262 @ bdfd596f](https://github.com/tc39/ecma262/blob/bdfd596f/CONTRIBUTING.md#L36-L39)

```markdown
TC39 is open to accepting new feature requests for ECMAScript, referred to as "proposals". Proposals go through a four-stage process which is documented in the [TC39 process document](https://tc39.es/process-document/).

Feature requests for future versions of ECMAScript should not be made in this repository. Instead, they are developed in separate GitHub repositories, which are then merged into the main repository once they have received "Stage 4".
```
