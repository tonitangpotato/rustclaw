# AI Sheet 有声书平台 - Design Document

## 项目概述

AI Sheet 是一个基于人工智能的有声书生成与管理平台，通过表格化的管理界面，实现从文本到有声读物的自动化生产流程。

### 核心价值主张

1. **智能化生产**：AI驱动的文本处理、语音合成、音频后期
2. **表格化管理**：直观的 Sheet 界面管理书籍、章节、任务状态
3. **自动化工作流**：从文本导入到成品发布的全流程自动化
4. **高质量输出**：多语言、多音色、情感表达的专业级语音

---

## 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                    AI Sheet Platform                         │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────────┐         ┌──────────────────┐          │
│  │   Web Frontend    │         │   Admin Panel    │          │
│  │                   │         │                  │          │
│  │  - Sheet UI       │         │  - User Mgmt     │          │
│  │  - Book Browser   │         │  - System Config │          │
│  │  - Player         │         │  - Analytics     │          │
│  └────────┬──────────┘         └────────┬─────────┘          │
│           │                             │                     │
│           └──────────┬──────────────────┘                     │
│                      │                                        │
│           ┌──────────▼──────────┐                            │
│           │    API Gateway       │                            │
│           │    (GraphQL/REST)    │                            │
│           └──────────┬───────────┘                            │
│                      │                                        │
│  ┌───────────────────┼───────────────────┐                   │
│  │                   │                   │                   │
│  ▼                   ▼                   ▼                   │
│ ┌─────────┐    ┌──────────┐      ┌──────────┐              │
│ │ Sheet   │    │  Book    │      │ Workflow │              │
│ │ Service │    │ Service  │      │ Engine   │              │
│ │         │    │          │      │          │              │
│ │ - CRUD  │    │ - Text   │      │ - Task   │              │
│ │ - View  │    │ - Meta   │      │   Queue  │              │
│ │ - Filter│    │ - Chapter│      │ - Status │              │
│ └────┬────┘    └────┬─────┘      └────┬─────┘              │
│      │              │                   │                    │
│  ┌───┴──────────────┴───────────────────┴───┐               │
│  │                                            │               │
│  │         AI Processing Pipeline            │               │
│  │                                            │               │
│  │  ┌──────────┐  ┌───────────┐  ┌────────┐│               │
│  │  │ Text     │→ │  Voice    │→ │ Audio  ││               │
│  │  │ Processor│  │ Synthesis │  │ Post   ││               │
│  │  └──────────┘  └───────────┘  └────────┘│               │
│  │       ↓              ↓             ↓     │               │
│  │  ┌──────────┐  ┌───────────┐  ┌────────┐│               │
│  │  │ - 分段   │  │ - TTS     │  │ - Mix  ││               │
│  │  │ - 清洗   │  │ - 音色    │  │ - EQ   ││               │
│  │  │ - 标注   │  │ - 情感    │  │ - 导出 ││               │
│  │  └──────────┘  └───────────┘  └────────┘│               │
│  └────────────────────────────────────────────┘               │
│                      │                                        │
│  ┌───────────────────┼────────────────────┐                  │
│  │                   │                    │                  │
│  ▼                   ▼                    ▼                  │
│ ┌─────────┐    ┌──────────┐      ┌──────────┐              │
│ │Database │    │ Storage  │      │  Cache   │              │
│ │         │    │          │      │          │              │
│ │PostgreSQL    │  S3/OSS  │      │  Redis   │              │
│ │         │    │          │      │          │              │
│ │- 元数据 │    │ - 音频   │      │ - 会话   │              │
│ │- 用户   │    │ - 文本   │      │ - 任务   │              │
│ │- 任务   │    │ - 封面   │      │          │              │
│ └─────────┘    └──────────┘      └──────────┘              │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

---

## 核心功能模块

### 1. Sheet 管理系统

**功能**：
- 📊 **书籍总表 (Books Sheet)**
  - 书名、作者、状态、进度、创建时间
  - 行操作：编辑、删除、导出、发布
  - 批量操作：批量导入、批量生成
  - 筛选排序：按状态、时间、作者筛选

- 📖 **章节表 (Chapters Sheet)**
  - 章节号、标题、字数、状态、音频时长
  - 关联父书籍
  - 段落分段预览
  - TTS 任务状态

- 🎙️ **语音配置表 (Voice Profiles)**
  - 音色名称、语言、性别、情感类型
  - 试听样本
  - 应用场景标签

- ⚙️ **任务队列表 (Task Queue)**
  - 任务类型、目标对象、状态、进度%
  - 错误日志
  - 重试机制

**技术实现**：
- 前端：类似 Google Sheets 的表格组件（如 AG-Grid、Handsontable）
- 后端：GraphQL API 提供灵活查询
- 实时更新：WebSocket 推送任务状态变化

---

### 2. 文本处理引擎

**功能模块**：

**2.1 文本导入**
- 支持格式：TXT, EPUB, DOCX, PDF
- 自动识别章节结构
- 元数据提取（书名、作者、目录）

**2.2 文本清洗**
```python
# 示例处理流程
def clean_text(raw_text):
    # 去除多余空白
    text = normalize_whitespace(raw_text)
    
    # 修正标点符号
    text = fix_punctuation(text)
    
    # 识别并处理对话
    text = dialogue_detection(text)
    
    # 繁简转换（可选）
    text = convert_simplified_traditional(text)
    
    return text
```

**2.3 智能分段**
- 基于 NLP 的语义分段
- 句子边界检测
- 合理控制段落长度（适配 TTS 输入限制）
- 对话识别与标注

**2.4 情感标注（可选）**
- 使用情感分析模型标注段落情绪
- 为 TTS 提供情感提示

---

### 3. 语音合成引擎

**TTS 方案选型**：

| 方案 | 优势 | 劣势 | 成本 |
|------|------|------|------|
| **Azure TTS** | 音质好、多语言、Neural Voice | 需付费 | $$$ |
| **Google Cloud TTS** | 稳定、WaveNet 质量高 | 价格较高 | $$$ |
| **ElevenLabs** | 极高质量、情感丰富 | API 限制、贵 | $$$$ |
| **OpenAI TTS** | 自然度高、成本适中 | 音色选择少 | $$ |
| **Coqui TTS** | 开源、可定制 | 需自部署、调优 | $ (计算) |
| **Edge TTS** | 免费、质量尚可 | 非官方、不稳定 | Free |

**推荐方案**：
- **生产级**：OpenAI TTS + Azure Neural Voice（主备）
- **开发测试**：Edge TTS
- **未来**：自训练 Coqui TTS 模型

**语音合成流程**：
```python
async def synthesize_chapter(chapter_id):
    chapter = await db.chapters.get(chapter_id)
    segments = chunk_text(chapter.text, max_length=4000)
    
    audio_files = []
    for i, segment in enumerate(segments):
        # 调用 TTS API
        audio = await tts_api.synthesize(
            text=segment.text,
            voice=chapter.voice_profile,
            emotion=segment.emotion,
            speed=1.0
        )
        
        # 保存音频片段
        file_path = f"audio/{chapter_id}/segment_{i}.mp3"
        await storage.save(file_path, audio)
        audio_files.append(file_path)
    
    # 合并音频
    final_audio = await merge_audio(audio_files)
    return final_audio
```

---

### 4. 音频后期处理

**功能模块**：

**4.1 音频拼接**
- 无缝合并多个 TTS 片段
- 处理段落间停顿（可配置时长）

**4.2 音质优化**
- 降噪处理
- 响度标准化（如 -16 LUFS）
- EQ 均衡

**4.3 背景音乐/音效（可选）**
- 片头片尾音乐
- 章节间过渡音效
- 背景环境音（低音量）

**4.4 格式转换**
- 输出格式：MP3, M4A, FLAC
- 码率：64kbps（省流）到 320kbps（高质）
- 元数据嵌入（ID3 tags）

**技术栈**：
- **FFmpeg**：音频处理核心
- **pydub** 或 **librosa**：Python 音频处理库

---

### 5. 工作流引擎

**任务类型**：
1. **文本导入任务**：上传 → 解析 → 入库
2. **TTS 任务**：文本 → 语音合成 → 保存
3. **音频处理任务**：拼接 → 优化 → 导出
4. **发布任务**：上传到 CDN/平台

**状态机**：
```
pending → processing → completed
                  ↓
              failed → retry (max 3次)
```

**任务调度**：
- 使用 Celery + Redis 作为分布式任务队列
- 支持任务优先级
- 失败重试 + 错误通知

**示例配置**：
```yaml
tasks:
  tts:
    max_workers: 4
    timeout: 300s
    retry:
      max_attempts: 3
      backoff: exponential
  
  audio_processing:
    max_workers: 2
    timeout: 600s
```

---

### 6. 用户与权限管理

**角色设计**：
- **管理员**：全部权限
- **编辑**：管理书籍、运行任务、查看报表
- **作者**：仅管理自己的书籍
- **查看者**：只读权限

**权限点**：
- `books.create`, `books.edit`, `books.delete`
- `tasks.run`, `tasks.cancel`
- `system.config`

---

### 7. 数据分析与报表

**统计维度**：
- 📊 书籍统计：总数、已完成、进行中
- 🎙️ 音频统计：总时长、平均时长、文件大小
- ⏱️ 任务统计：成功率、平均处理时间、失败原因分布
- 💰 成本统计：TTS API 调用次数、费用估算

**可视化**：
- Dashboard：使用 Chart.js 或 ECharts
- 趋势图：每日生成量、成功率走势

---

## 数据模型

### Books（书籍表）
```sql
CREATE TABLE books (
    id UUID PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    author VARCHAR(255),
    description TEXT,
    cover_url VARCHAR(500),
    language VARCHAR(10) DEFAULT 'zh-CN',
    status VARCHAR(20) DEFAULT 'draft',
    -- draft, processing, completed, published
    total_chapters INT DEFAULT 0,
    total_duration_seconds INT DEFAULT 0,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_books_status ON books(status);
CREATE INDEX idx_books_created_by ON books(created_by);
```

### Chapters（章节表）
```sql
CREATE TABLE chapters (
    id UUID PRIMARY KEY,
    book_id UUID REFERENCES books(id) ON DELETE CASCADE,
    chapter_number INT NOT NULL,
    title VARCHAR(255) NOT NULL,
    text_content TEXT NOT NULL,
    word_count INT,
    audio_url VARCHAR(500),
    duration_seconds INT,
    voice_profile_id UUID REFERENCES voice_profiles(id),
    status VARCHAR(20) DEFAULT 'pending',
    -- pending, processing, completed, failed
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    
    UNIQUE(book_id, chapter_number)
);

CREATE INDEX idx_chapters_book_id ON chapters(book_id);
CREATE INDEX idx_chapters_status ON chapters(status);
```

### Voice Profiles（音色配置）
```sql
CREATE TABLE voice_profiles (
    id UUID PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    provider VARCHAR(50) NOT NULL, -- 'openai', 'azure', 'elevenlabs'
    voice_id VARCHAR(100) NOT NULL,
    language VARCHAR(10) NOT NULL,
    gender VARCHAR(10),
    description TEXT,
    sample_url VARCHAR(500),
    settings JSONB, -- provider-specific settings
    created_at TIMESTAMP DEFAULT NOW()
);
```

### Tasks（任务表）
```sql
CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    type VARCHAR(50) NOT NULL, -- 'tts', 'audio_merge', 'publish'
    target_type VARCHAR(50) NOT NULL, -- 'book', 'chapter'
    target_id UUID NOT NULL,
    status VARCHAR(20) DEFAULT 'pending',
    progress INT DEFAULT 0, -- 0-100
    error_message TEXT,
    retry_count INT DEFAULT 0,
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_target ON tasks(target_type, target_id);
```

---

## 技术栈

### 后端
- **语言**：Python 3.11+ 或 Rust (RustClaw 集成)
- **框架**：
  - Python: FastAPI / Django
  - Rust: Axum + SQLx
- **数据库**：PostgreSQL 15+
- **缓存**：Redis 7+
- **任务队列**：Celery (Python) 或 内置 tokio task (Rust)
- **对象存储**：AWS S3 / MinIO / 阿里云 OSS

### 前端
- **框架**：React 18 + TypeScript
- **UI 库**：
  - Material-UI 或 Ant Design
  - AG-Grid (表格组件)
- **状态管理**：Zustand / Jotai
- **数据获取**：React Query + GraphQL

### AI/ML
- **TTS API**：OpenAI TTS, Azure Cognitive Services
- **NLP**：spaCy, Hugging Face Transformers
- **音频处理**：FFmpeg, pydub

### DevOps
- **容器化**：Docker + Docker Compose
- **部署**：Kubernetes / Railway / Fly.io
- **CI/CD**：GitHub Actions
- **监控**：Prometheus + Grafana

---

## MVP 实现路线

### Phase 1: 核心功能（2-3 周）
- ✅ 书籍与章节 CRUD API
- ✅ 文本导入（支持 TXT）
- ✅ 基础 TTS 集成（OpenAI）
- ✅ Sheet UI（基础表格）
- ✅ 任务队列（简单同步执行）

### Phase 2: 增强功能（2-3 周）
- ✅ 音频后期处理（拼接、导出）
- ✅ 多音色支持
- ✅ 任务管理 UI
- ✅ 用户权限系统
- ✅ 对象存储集成

### Phase 3: 高级功能（3-4 周）
- ✅ 智能分段算法
- ✅ 情感标注
- ✅ 批量处理
- ✅ 数据分析 Dashboard
- ✅ 导出多种格式

### Phase 4: 优化与扩展（持续）
- 🔄 性能优化
- 🔄 多语言支持
- 🔄 自定义 TTS 模型
- 🔄 API 开放平台

---

## 成本估算

### 开发成本
- 后端开发：2-3 人月
- 前端开发：1.5-2 人月
- 测试与优化：1 人月
- **总计**：约 4.5-6 人月

### 运营成本（月）
- 服务器（2核4G）：$20
- 数据库（PostgreSQL）：$15
- 对象存储（100GB）：$3
- **TTS API**（关键成本）：
  - OpenAI: $0.015/1K chars
  - 假设每月生成 100 本书，每本 10 万字 = 1000 万字
  - 成本：$150/月

**总计**：约 $188/月（不含 CDN 流量）

---

## 风险与挑战

### 技术风险
1. **TTS 质量不稳定**：不同 API 输出质量差异大
   - 缓解：多供应商备份方案
2. **音频处理性能**：大文件处理慢
   - 缓解：异步队列 + 分段处理
3. **存储成本**：音频文件占用空间大
   - 缓解：压缩算法 + 冷热存储分离

### 业务风险
1. **版权问题**：用户上传盗版书籍
   - 缓解：用户协议 + 内容审核
2. **API 费用失控**：TTS 调用量过大
   - 缓解：配额限制 + 预算告警

---

## 未来展望

### 短期（6 个月）
- 支持主流电子书格式（EPUB, PDF）
- 移动端 App（iOS/Android）
- 社区功能（书籍分享、评论）

### 中期（1 年）
- 自训练 TTS 模型（降低成本）
- 多角色配音（对话自动分配音色）
- AI 封面生成

### 长期（2 年+）
- 实时语音克隆
- 多模态内容生成（音频 + 图片 + 视频）
- 开放 API 平台

---

## 附录

### A. 竞品分析

| 产品 | 特点 | 优势 | 劣势 |
|------|------|------|------|
| **讯飞配音** | 成熟的中文 TTS | 音质好、稳定 | 价格贵、不开放 |
| **剪映** | 视频配音为主 | 免费、易用 | 非专业有声书场景 |
| **Descript** | 国外产品 | 功能强大 | 中文支持差 |
| **AI Sheet** | *本产品* | 表格化管理、全流程自动化 | 新产品 |

### B. 参考资源
- [Azure Neural Voice](https://azure.microsoft.com/services/cognitive-services/text-to-speech/)
- [OpenAI TTS API](https://platform.openai.com/docs/guides/text-to-speech)
- [Coqui TTS (开源)](https://github.com/coqui-ai/TTS)
- [AG-Grid (表格组件)](https://www.ag-grid.com/)

---

**文档版本**：v1.0  
**创建日期**：2026-03-27  
**作者**：AI Assistant (Based on RustClaw)  
**状态**：Draft
