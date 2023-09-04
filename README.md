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

* Start Segment Address and Start Linear Address have no effect on analysis.
* Any invalid line will be ignored (IE a line with a bad checksum)
* Segments must align to a full character - a single character cannot represent two segments.
* EOF records are not currently supported. Visual blocks are treated as existing to the end of the last segment with data records.
* All output assumes a monospace font