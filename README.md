<div align="center">
<h1>Hopp - Open Source pair programming app</h1>

<img src="./docs/src/assets/banner.png" alt="Hopp" />

[![Discord](https://img.shields.io/discord/1348693269013467167?color=7289da&label=Discord&logo=discord&logoColor=ffffff)](https://discord.gg/TKRpS3aMn9)
![Powered by LiveKit](https://img.shields.io/badge/powered-by%20LiveKit-blue.svg?labelColor=212121&logo=data:image/svg%2bxml;base64,PHN2ZyB3aWR0aD0iMjQiIGhlaWdodD0iMjQiIHZpZXdCb3g9IjAgMCAyNCAyNCIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPGcgY2xpcC1wYXRoPSJ1cmwoI2NsaXAwXzQyODRfMzM1ODUpIj4KPHBhdGggZD0iTTE0LjQwMDQgOS41OTk2MUg5LjU5OTYxVjE0LjQwMDRIMTQuNDAwNFY5LjU5OTYxWiIgZmlsbD0id2hpdGUiLz4KPHBhdGggZD0iTTE5LjIwMTEgNC44MDA3OEgxNC40MDA0VjkuNjAxNTNIMTkuMjAxMVY0LjgwMDc4WiIgZmlsbD0id2hpdGUiLz4KPHBhdGggZD0iTTE5LjIwMTEgMTQuNDAwNEgxNC40MDA0VjE5LjIwMTFIMTkuMjAxMVYxNC40MDA0WiIgZmlsbD0id2hpdGUiLz4KPHBhdGggZD0iTTI0IDBIMTkuMTk5MlY0LjgwMDc1SDI0VjBaIiBmaWxsPSJ3aGl0ZSIvPgo8cGF0aCBkPSJNMjQgMTkuMTk5MkgxOS4xOTkyVjI0SDI0VjE5LjE5OTJaIiBmaWxsPSJ3aGl0ZSIvPgo8cGF0aCBkPSJNNC44MDA3NSAxOS4xOTkyVjE0LjQwMDRWOS41OTk2MlY0LjgwMDc1VjBIMFY0LjgwMDc1VjkuNTk5NjJWMTQuNDAwNFYxOS4xOTkyVjI0SDQuODAwNzVIOS41OTk2M0gxNC40MDA0VjE5LjE5OTJIOS41OTk2M0g0LjgwMDc1WiIgZmlsbD0id2hpdGUiLz4KPC9nPgo8ZGVmcz4KPGNsaXBQYXRoIGlkPSJjbGlwMF80Mjg0XzMzNTg1Ij4KPHJlY3Qgd2lkdGg9IjI0IiBoZWlnaHQ9IjI0IiBmaWxsPSJ3aGl0ZSIvPgo8L2NsaXBQYXRoPgo8L2RlZnM+Cjwvc3ZnPgo=)
[![License](https://img.shields.io/github/license/gethopp/hopp)](https://github.com/gethopp/hopp/blob/main/LICENSE.md)
[![Pairing with Hopp](https://gethopp.app/git/hopp-shield.svg?ref=hopp-repo)](https://gethopp.app)

</div>

Hopp is an open source pair programming app that allows you to pair program with your teammates. The app is built with Tauri, and the WebRTC infrastructure is powered by [LiveKit](https://livekit.io).

## Features

- **⚡ Super high quality screen sharing**
  - [We optimised WebRTC](https://gethopp.app/blog/latency-exploration) to get the best quality screen sharing
  - [Rely on LiveKit's network](https://docs.livekit.io/home/cloud/architecture/#distributed-mesh-architecture) for low latency at scale
- **👥👥 Mob programming**
  - Join a room and start pairing immediately with up to 10 teammates
- **🔗 One click pairing**
  - No more sharing links with your teammates on chat
- **🪟 Built in the open**
  - We want to build Hopp with the OSS community
  - This comes with benefits as self-hosting, and innovation from the community

## 🛠️ Tech Stack

### Backend

- [Go](https://go.dev/) - API server
- [PostgreSQL](https://www.postgresql.org/) - Data storage

### Frontend

- [React](https://react.dev/) with [TypeScript](https://www.typescriptlang.org/) - Web app and Desktop app
- [TailwindCSS](https://tailwindcss.com/) - Styling

### Desktop App

- [Tauri](https://tauri.app/) - Desktop app
- [Rust](https://www.rust-lang.org/) - Desktop app (core process)

## 📚 Documentation

- [Official Documentation](https://docs.gethopp.app)
- [Core process docs](/core/README.md)

## 💻 Local development

- [Prerequisites](https://docs.gethopp.app/quick-start/local-development/prerequisites/)
- [Development workflow](https://docs.gethopp.app/quick-start/local-development/development-workflow/)

## 🌐 Community & Support

- Join our [Discord community](https://discord.gg/TKRpS3aMn9)
- Follow us on [Twitter](https://x.com/gethopp_app)
