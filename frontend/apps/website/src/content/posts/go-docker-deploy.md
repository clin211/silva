---
title: 基于 GitHub Actions 与 Docker 的 Go 项目部署实战
summary: 把构建、镜像推送、健康检查和回滚串进一条可重复的交付流程。
published: '2026-11-18'
updated: '2026-11-20'
category: backend
tags: [go, docker, github-actions]
featured: true
cover:
    color: deep
    mark: CI/CD
---

为 Go 服务搭建交付流程时，先明确远端要运行哪个产物、怎样确认它就绪，以及故障时回到哪个版本。这几个问题决定了流水线需要记录什么。

## 一、整体流程

1. 提交合并后，CI 执行测试，构建并推送镜像。
2. 登记本次提交 SHA 与镜像摘要的对应关系。
3. 部署任务读取记录，拉取明确指定的镜像，等待健康检查。
4. 健康检查通过后，才把本次镜像登记为已部署版本。

建议把镜像地址、部署环境和摘要放在同一份交付记录里。值班同学能从一条记录拿到排查或回退所需的上下文。

## 二、多阶段构建

编译阶段保留工具链和依赖缓存。运行阶段只复制构建结果，CI 对两个基础镜像都固定版本。下面的片段假设应用入口是 `./cmd/service`。

```dockerfile
FROM golang:1.25-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -trimpath -o /out/service ./cmd/service

FROM alpine:3.22
WORKDIR /app
COPY --from=build /out/service /app/service
USER 65532:65532
ENTRYPOINT ["/app/service"]
```

### 缓存依赖层

先复制模块描述，再复制源码。只修改业务代码时，依赖下载层仍能复用。遇到跨架构构建，记得分别验证目标架构的运行产物。

## 三、GitHub Actions 工作流

给测试、构建和部署分别定义任务。部署依赖构建任务的镜像摘要，并按环境串行化，避免两个提交同时修改运行版本。生产凭证只提供给声明了对应环境的部署任务。

## 四、回滚

保留上一份经健康检查确认的镜像摘要。回滚时重新部署该摘要，检查服务就绪和一个代表性只读请求。数据库有迁移时，先确认旧版本是否仍能读取当前结构。

## 小结

一次演练至少覆盖镜像无法拉取、服务启动后未就绪，以及应用成功回退后数据仍能正常读取。演练的记录可以直接补进部署手册。
