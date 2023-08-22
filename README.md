A visualizer for Intel Hex files. Good for tracking space allocations in resource constrained firmware images.

## Running ihex-visualize

ihex-visualize can be run in its default configuration with

```
ihex-visualize -f file.hex
```

Full options can be seen by running
```
ihex-visualize --help
```

## Limitations

* The parsing supports Extended Segment Address directives but not Extended Linear Address. Start Segment Address and Start Linear Address have no effect on analysis.
* A data line wrapping around the end of a segment block is not supported at this time.
* Any invalid line will be ignored (IE a line with a bad checksum)
* Segments must align to a full character - a single character cannot represent two segments.
