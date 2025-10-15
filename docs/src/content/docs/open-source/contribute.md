---
title: How to Contribute
description: Learn how to contribute to the Hopp open-source project.
---

Hopp is an open-source project and we welcome contributions from the community! Whether you're fixing bugs, adding features, improving documentation, or helping with design, your contributions make Hopp better for everyone.

## Getting Started

### 1. Set Up Development Environment

Follow our [Local Development Guide](/quick-start/local-development/development-workflow/).

### 2. Find Something to Work On

#### Good First Issues

Check issues [labeled `Good first issue`](https://github.com/gethopp/hopp/issues?q=is%3Aissue%20state%3Aopen%20label%3A%22Good%20first%20issue%22) on GitHub

#### Specific Issues

Additionally we split the issues into different categories, so you can find something that fits your skills and interests.

A partial list of labels we use:

- `App` - For issues related to the desktop app (Tauri)
- `Backend` - For issues related to the backend (Go)
- `Core` - For issues related to the core (Rust)
- `Docs` - For issues related to the documentation
- `Frontend` - For issues related to the web-app (React)
- `GoLang`, `Rust`, `JavaScript` - Self explanatory

### 3. Before You Start

- Comment on the issue you want to work on
- Ask questions if anything is unclear
- Check if someone else is already working on it
- Discuss your approach for larger changes

If you want to direct communication with us, you can join our [Discord channel](https://discord.gg/TKRpS3aMn9).

## Contribution Process

### 1. Fork and Clone

```bash
# Fork the repository on GitHub, then clone your fork
git clone https://github.com/YOUR-USERNAME/hopp.git
cd hopp
```

### 2. Install pre-commit hooks

This will avoid any painful last minute breaking CI in your PR.

Before installing our hooks you should install [pre-commit](https://pre-commit.com/#install).

```bash
pre-commit install
```

### 3. Create a Branch

```bash
# Create a new branch for your feature/fix/bug
git checkout -b feature/your-feature-name
```

### 4. Make Your Changes

- Write clean, well-documented code
- Follow the existing code style
- Add tests for new functionality if needed
- Update documentation as needed

### 5. Test Your Changes

```bash
# Run the test suite
# To be added!

# Test manually with different scenarios
# Ensure your changes don't break existing functionality
```

### 6. Commit Your Changes

Write clear, descriptive commit messages 🙏. We try to use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for commit messages.

### 7. Push and Create Pull Request

```bash
# Push your branch to your fork
git push origin feature/your-feature-name

# Create a pull request on GitHub
# Fill out the PR template with details about your changes
```

## Getting Help

If you need help:

1. **Check Existing Issues**: Your question might already be answered
2. **Read Documentation**: Check guides and API docs
3. **Join Our Chat**: Real-time help from contributors in our [Discord channel](https://discord.gg/TKRpS3aMn9)
