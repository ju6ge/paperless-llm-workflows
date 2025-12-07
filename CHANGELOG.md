# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- increase default num gpu layers to 1024 for better performance with gpu
- updated llama-cpp bindings to version b7314 2025-12-07

## [0.3.1] - 2025-11-26

### Added
- new api endpoint to enable decision based workflows

## [0.3.0] - 2025-11-25

- Rename Project to `paperless-llm-workflows`

### Added
- Added openapi specs for workflow trigger server endpoints
- Added `next_tag` parameter to Webhook endpoints, allowing better workflow stages
- Changed Architecture to receive webhooks to trigger document processing
- decouple sending updated documents to paperless from llm processing pipeline
- feature correspondent suggestion, let this software suggest correspondents

### Fixed
- better error handling for model generation errors
- support bigger context windows for larger documents
- better model sampling pipeline, punish duplicate generations


## [0.2.1] - 2025-11-06

### Removed 
- disable generation of alternative value fields (currently unsed feature anyway)

## [0.2.0] - 2025-11-02

### Added
- better guiding of the model for `select` and `date` custom fields

## [0.1.2] - 2025-10-30

### Added
- expand documentation on containerized usage

### Fixed
- server configuration

## [0.1.1] - 2025-10-30

### Fixed
- do not tag documents when running in `dry-run` mode
- limit decimal for currency custom fields to 2 decimal places 

## [0.1.0] - 2025-10-29

Initial Release



