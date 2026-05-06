# syntax=docker/dockerfile:1.7

FROM --platform=$BUILDPLATFORM golang:1.25-bookworm AS builder
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
ARG TARGETOS
ARG TARGETARCH
RUN CGO_ENABLED=0 GOOS=$TARGETOS GOARCH=$TARGETARCH \
    go build -ldflags="-s -w" -o /out/server ./main.go

FROM gcr.io/distroless/static-debian12
WORKDIR /app
COPY --from=builder /out/server .
COPY web ./web
ENV USE_TLS=false
EXPOSE 1926
CMD ["./server"]
