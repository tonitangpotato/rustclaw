---
name: research
description: Competitive analysis, technical research, and market validation for new features
version: "1.0.0"
author: potato
triggers:
  patterns:
    - "research"
    - "竞品"
    - "调研"
    - "market research"
    - "competitive analysis"
tags:
  - research
  - analysis
  - strategy
priority: 60
always_load: false
max_body_size: 4096
---
# SKILL: Research Phase

> Conduct competitive analysis, technical research, and market validation before drafting requirements.

## Trigger Conditions

This skill activates when:
- Starting a new feature that needs research
- User asks for "research", "竞品", "调研", "market research", or "competitive analysis"
- GID ritual reaches the research phase

## Input

- Feature name/description from capture-idea phase
- Any existing context from `.gid/features/{feature}/idea.md`

## Output

Create `docs/RESEARCH-{feature-name}.md` with the following structure:

```markdown
# Research: {feature-name}

## Executive Summary
2-3 sentence overview of findings and recommendation.

## Competitive Landscape

### {Competitor 1}
- **URL**: 
- **What it does**: 
- **Key features**: 
- **Strengths**: 
- **Weaknesses**: 
- **Pricing**: (if applicable)
- **User sentiment**: (from reviews, forums, etc.)

### {Competitor 2}
...

### Competitive Matrix
| Feature | Us (planned) | Competitor 1 | Competitor 2 |
|---------|--------------|--------------|--------------|
| ...     | ...          | ...          | ...          |

## Technical Approaches

### Option A: {approach name}
- **Description**: 
- **Pros**: 
- **Cons**: 
- **Effort estimate**: 
- **Key libraries/dependencies**: 

### Option B: {approach name}
...

### Technical Recommendation
Which approach to use and why.

## Market Signals

### User Demand Evidence
- Forum discussions, Reddit threads, HN comments
- GitHub issues/stars on related projects
- Search volume trends (if available)

### Pain Points Found
- What problems users are reporting
- What's missing in existing solutions

### Market Size Indicators
- Target audience size estimates
- Growth trends in the space

## Prior Art

### Open Source Projects
- Relevant repos with brief assessment

### Academic Papers / Blog Posts
- Key technical resources that inform the design

### Lessons Learned from Others
- What worked/failed for similar projects

## Key Insights

1. **Insight 1**: [actionable takeaway]
2. **Insight 2**: [actionable takeaway]
3. **Insight 3**: [actionable takeaway]
4. **Insight 4**: [actionable takeaway]
5. **Insight 5**: [actionable takeaway]

## Recommendation

### Go / No-Go Assessment
**Recommendation**: [GO / NO-GO / PIVOT]

**Reasoning**:
- [Key factor 1]
- [Key factor 2]
- [Key factor 3]

### If GO, Key Success Factors
- What must be true for this to succeed
- What differentiates us from competition

### If NO-GO, Alternatives
- What should we do instead
```

## Research Process

### Step 0: Codebase Discovery (MANDATORY before any research)
```
→ find . -type f \( -name "*.rs" -o -name "*.ts" -o -name "*.py" \) | xargs grep -li "{feature_keywords}" 2>/dev/null
→ find . -path "*/packages/*" -o -path "*/crates/*" -o -path "*/src/*" | grep -i "{feature_name}"
→ Check if implementation already exists under a different name/path
→ If files found:
   - Read them to understand what's already built
   - Document in research output: "## Existing Implementation" section
   - Assess: is this task already done? partially done? needs different approach?
   - If fully implemented: STOP research, report "already exists at {path}"
→ If nothing found: proceed to Step 1
```
**Why this step exists**: We once reimplemented a TypeScript MCP server in Rust because we didn't check `packages/mcp/` first. This step is free (no LLM, just grep) and prevents wasted work.

### Step 1: Competitive Analysis
```
→ web_search("{feature} alternatives")
→ web_search("{feature} competitors")
→ web_search("{feature} open source")
→ For each competitor found:
   → web_fetch(competitor_url) to understand features
   → web_search("{competitor} reviews") for user sentiment
```

### Step 2: Technical Research
```
→ web_search("{feature} implementation")
→ web_search("{feature} architecture")
→ web_search("{feature} rust library" or "{feature} python library")
→ Check GitHub trending/search for relevant projects
→ Evaluate feasibility based on existing tools/libraries
```

### Step 3: Market Validation
```
→ web_search("{feature} demand")
→ web_search("{feature} reddit" or "{feature} hacker news")
→ web_search("{feature} problems" or "{feature} pain points")
→ Look for patterns in user complaints/requests
```

### Step 4: Prior Art
```
→ web_search("{feature} paper" or "{feature} research")
→ web_search("{feature} blog post" or "{feature} tutorial")
→ GitHub search for related projects, check their issues/discussions
```

### Step 5: Synthesize & Recommend
```
→ Compile findings into structured document
→ Identify patterns across sources
→ Make go/no-go recommendation with clear reasoning
→ If GO, identify key differentiators and success factors
```

## Notes

- **Speed over perfection** — This is research to inform decisions, not an exhaustive academic survey
- **Focus on actionable insights** — Every section should help make better product decisions
- **Be honest about unknowns** — If data is scarce, say so rather than speculating
- **Quantify when possible** — GitHub stars, forum upvotes, search trends > vague assertions
- **Document sources** — Include URLs so findings can be verified
- **Time-box** — Aim for 20-30 minutes of research, not hours
- **Use 中英混用 naturally** if researching Chinese market/competitors
